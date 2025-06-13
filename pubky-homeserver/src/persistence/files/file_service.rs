use crate::{
    persistence::{
        files::entry_service::EntryService,
        lmdb::{
            tables::files::{Entry, FileLocation, InDbFileId},
            LmDB,
        },
    },
    shared::webdav::EntryPath,
    ConfigToml,
};
use bytes::Bytes;
use futures_util::Stream;
#[cfg(test)]
use futures_util::StreamExt;
#[cfg(test)]
use opendal::Buffer;
use std::path::Path;

use super::{FileIoError, FileStream, OpendalService, WriteStreamError};

/// The file service creates an abstraction layer over the LMDB and OpenDAL services.
/// This way, files can be managed in a unified way.
#[derive(Debug, Clone)]
pub struct FileService {
    pub(crate) entry_service: EntryService,
    pub(crate) opendal_service: OpendalService,
    pub(crate) db: LmDB,
    user_quota_bytes: Option<u64>,
}

impl FileService {
    pub fn new(opendal_service: OpendalService, db: LmDB, user_quota_bytes: Option<u64>) -> Self {
        Self {
            entry_service: EntryService::new(db.clone(), user_quota_bytes),
            opendal_service,
            db,
            user_quota_bytes,
        }
    }

    pub fn new_from_config(
        config: &ConfigToml,
        data_directory: &Path,
        db: LmDB,
    ) -> Result<Self, FileIoError> {
        let opendal_service = OpendalService::new_from_config(&config.storage, data_directory)?;
        let quota_mb = config.general.user_storage_quota_mb;
        let quota_bytes = if quota_mb == 0 {
            None
        } else {
            Some(quota_mb * 1024 * 1024)
        };
        Ok(Self::new(opendal_service, db, quota_bytes))
    }

    #[cfg(test)]
    pub fn test(db: LmDB) -> Self {
        use crate::storage_config::StorageConfigToml;

        let storage_config = StorageConfigToml::InMemory;
        let opendal_service =
            OpendalService::new_from_config(&storage_config, Path::new("/tmp/test"))
                .expect("Failed to create OpenDAL service for testing");
        Self::new(opendal_service, db, None)
    }

    /// Get the metadata of a file.
    pub async fn get_info(&self, path: &EntryPath) -> Result<Entry, FileIoError> {
        self.db.get_entry(path)
    }

    /// Get the content of a file as bytes.
    /// Errors if the file does not exist.
    #[cfg(test)]
    pub async fn get(&self, path: &EntryPath) -> Result<Bytes, FileIoError> {
        let mut stream = self.get_stream(path).await?;
        let mut collected_data = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            collected_data.extend_from_slice(&chunk);
        }

        Ok(Bytes::from(collected_data))
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked.
    /// Errors if the file does not exist.
    pub async fn get_stream(&self, path: &EntryPath) -> Result<FileStream, FileIoError> {
        let entry = self.get_info(path).await?;
        let stream: FileStream = match entry.file_location() {
            FileLocation::LmDB => {
                let temp_file = self.db.read_file(&entry.file_id()).await?;
                temp_file.as_stream()?
            }
            FileLocation::OpenDal => self.opendal_service.get_stream(path).await?,
        };
        Ok(stream)
    }

    /// Get the remaining quota bytes for a user.
    /// Returns `u64::MAX` if the user has an unlimited quota.
    fn get_user_quota_bytes_allowance(&self, path: &EntryPath) -> Result<Option<u64>, FileIoError> {
        let max_limit = match self.user_quota_bytes {
            Some(limit) => limit,
            None => return Ok(None),
        };
        let current_usage_bytes = match self.db.get_user_data_usage(path.pubkey())? {
            Some(usage) => usage,
            None => return Err(FileIoError::NotFound),
        };
        let existing_entry_content_length = self.db.get_entry_content_length_default_zero(path)?;

        let remaining_quota_bytes = max_limit
            .saturating_add(existing_entry_content_length)
            .saturating_sub(current_usage_bytes);
        Ok(Some(remaining_quota_bytes))
    }

    /// Write a file to the database and storage depending on the selected target location.
    #[cfg(test)]
    pub async fn write(
        &self,
        path: &EntryPath,
        data: Buffer,
        location: FileLocation,
    ) -> Result<Entry, FileIoError> {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from(data.to_vec()))]);
        let entry = self.write_stream(path, location, stream).await?;
        Ok(entry)
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write_stream(
        &self,
        path: &EntryPath,
        location: FileLocation,
        stream: impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send,
    ) -> Result<Entry, FileIoError> {
        let remaining_bytes_usage = self.get_user_quota_bytes_allowance(path)?;

        let metadata = match location {
            FileLocation::LmDB => {
                self.db
                    .write_file_from_stream(path, stream, remaining_bytes_usage)
                    .await?
            }
            FileLocation::OpenDal => {
                self.opendal_service
                    .write_stream(path, stream, remaining_bytes_usage)
                    .await?
            }
        };

        let write_result = self
            .entry_service
            .write_entry(path, &metadata, location.clone());
        if write_result.is_err() {
            // Writing the entry failed. Delete the file in storage and return the error.
            match location {
                FileLocation::LmDB => {
                    let mut wtxn = self.db.env.write_txn()?;
                    let fileid = InDbFileId(metadata.modified_at);
                    self.db.delete_file(&fileid, &mut wtxn)?;
                    wtxn.commit()?;
                }
                FileLocation::OpenDal => {
                    self.opendal_service.delete(path).await?;
                }
            };
        };

        write_result
    }

    /// Delete a file.
    pub async fn delete(&self, path: &EntryPath) -> Result<(), FileIoError> {
        let entry = self.get_info(path).await?;
        self.entry_service.delete_entry(path)?;
        match entry.file_location() {
            FileLocation::LmDB => {
                let mut wtxn = self.db.env.write_txn()?;
                self.db.delete_file(&entry.file_id(), &mut wtxn)?;
                wtxn.commit()?;
            }
            FileLocation::OpenDal => {
                self.opendal_service.delete(path).await?;
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures_lite::StreamExt;

    use crate::{shared::webdav::WebDavPath, storage_config::StorageConfigToml};

    use super::*;

    #[tokio::test]
    async fn test_write_get_delete_lmdb_and_opendal() {
        let mut config = ConfigToml::test();
        config.storage = StorageConfigToml::InMemory;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        // User should not have any data usage yet
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), Some(0));

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());

        // Test getting a non-existent file
        match file_service.get_stream(&path).await {
            Ok(_) => panic!("Should error for non-existent file"),
            Err(FileIoError::NotFound) => {}
            Err(e) => panic!("Should error for non-existent file: {}", e),
        };

        // Test data
        let test_data = b"Hello, world! This is test data for the get method.";
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);

        // Test LMDB
        let entry = file_service
            .write_stream(&path, FileLocation::LmDB, stream)
            .await
            .unwrap();
        assert_eq!(*entry.file_location(), FileLocation::LmDB);
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64),
            "Data usage should be the size of the file"
        );

        // Get the file content and verify
        let mut stream = file_service
            .get_stream(&path)
            .await
            .expect("File should exist");
        let mut collected_data = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected_data.extend_from_slice(&chunk);
        }

        assert_eq!(
            collected_data,
            test_data.to_vec(),
            "Content should match original data for LMDB location"
        );

        file_service.delete(&path).await.unwrap();
        let result = file_service.get_stream(&path).await;
        assert!(result.is_err(), "Should error for deleted file");
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(0),
            "Data usage should be 0 after deleting file"
        );

        // Test OpenDal location
        let path = EntryPath::new(
            pubkey.clone(),
            WebDavPath::new("/test_opendal.txt").unwrap(),
        );
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);
        let entry = file_service
            .write_stream(&path, FileLocation::OpenDal, stream)
            .await
            .unwrap();
        assert_eq!(*entry.file_location(), FileLocation::OpenDal);
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64),
            "Data usage should be the size of the file"
        );

        // Get the file content and verify
        let mut stream = file_service
            .get_stream(&path)
            .await
            .expect("File should exist");
        let mut collected_data = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected_data.extend_from_slice(&chunk);
        }

        assert_eq!(
            collected_data,
            test_data.to_vec(),
            "Content should match original data for OpenDal location"
        );

        // Clean up
        file_service.delete(&path).await.unwrap();
        let result = file_service.get_stream(&path).await;
        assert!(result.is_err(), "Should error for deleted file");
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(0),
            "Data usage should be 0 after deleting file"
        );
    }

    #[tokio::test]
    async fn test_write_get_basic() {
        let mut config = ConfigToml::test();
        config.storage = StorageConfigToml::InMemory;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let test_data = b"Hello, world!";
        let buffer = Buffer::from(test_data.as_slice());

        // Test LMDB
        let lmdb_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        file_service
            .write(&lmdb_path, buffer.clone(), FileLocation::LmDB)
            .await
            .unwrap();
        let content = file_service.get(&lmdb_path).await.unwrap();
        assert_eq!(content.as_ref(), test_data);

        // Test OpenDal
        let opendal_path = EntryPath::new(pubkey, WebDavPath::new("/test_opendal.txt").unwrap());
        file_service
            .write(&opendal_path, buffer, FileLocation::OpenDal)
            .await
            .unwrap();
        let content = file_service.get(&opendal_path).await.unwrap();
        assert_eq!(content.as_ref(), test_data);
    }

    #[tokio::test]
    async fn test_data_usage_update_basic() {
        let mut config = ConfigToml::test();
        config.general.user_storage_quota_mb = 1;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service
            .write(&path, buffer, FileLocation::OpenDal)
            .await
            .unwrap();
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64)
        );

        // Delete the file and check if the data usage is updated correctly.
        file_service.delete(&path).await.unwrap();
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), Some(0));
    }

    /// Override and existing entry and check if the data usage is updated correctly.
    #[tokio::test]
    async fn test_data_usage_override_existing_entry() {
        let mut config = ConfigToml::test();
        config.general.user_storage_quota_mb = 1;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service
            .write(&path, buffer, FileLocation::OpenDal)
            .await
            .unwrap();

        let test_data2 = vec![2u8; 1024];
        let buffer2 = Buffer::from(test_data2.clone());
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());

        file_service
            .write(&path, buffer2, FileLocation::OpenDal)
            .await
            .unwrap();

        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data2.len() as u64)
        );
    }

    /// Write a file that is exactly at the quota and check if the data usage is updated correctly.
    #[tokio::test]
    async fn test_data_usage_exactly_to_quota() {
        let mut config = ConfigToml::test();
        config.general.user_storage_quota_mb = 1;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024 * 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service
            .write(&path, buffer, FileLocation::OpenDal)
            .await
            .unwrap();

        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64)
        );
    }

    #[tokio::test]
    async fn test_data_usage_above_quota() {
        let mut config = ConfigToml::test();
        config.general.user_storage_quota_mb = 1;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024 * 1024 + 1];
        let buffer = Buffer::from(test_data.clone());

        match file_service
            .write(&path, buffer, FileLocation::OpenDal)
            .await
        {
            Ok(_) => panic!("Should error for file above quota"),
            Err(FileIoError::DiskSpaceQuotaExceeded) => {} // All good
            Err(e) => {
                panic!("Should error for file above quota: {:?}", e);
            }
        }

        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), Some(0));
    }

    /// Override and existing entry and check if the data usage is updated correctly.
    #[tokio::test]
    async fn test_data_usage_override_existing_above_quota() {
        let mut config = ConfigToml::test();
        config.general.user_storage_quota_mb = 1;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone())
                .expect("Failed to create file service for testing");

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service
            .write(&path, buffer, FileLocation::OpenDal)
            .await
            .unwrap();

        let test_data2 = vec![2u8; 1024 * 1024 + 1];
        let buffer2 = Buffer::from(test_data2.clone());
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());

        match file_service
            .write(&path, buffer2, FileLocation::OpenDal)
            .await
        {
            Ok(_) => panic!("Should error for file above quota"),
            Err(FileIoError::DiskSpaceQuotaExceeded) => {} // All good
            Err(e) => {
                panic!("Should error for file above quota: {:?}", e);
            }
        }

        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64)
        );
    }
}
