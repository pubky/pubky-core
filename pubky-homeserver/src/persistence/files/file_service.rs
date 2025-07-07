#[cfg(test)]
use crate::AppContext;
use crate::{
    persistence::{
        files::entry_service::EntryService,
        lmdb::{tables::entries::Entry, LmDB},
    },
    shared::webdav::EntryPath, ConfigToml,
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
}

impl FileService {
    pub fn new(opendal_service: OpendalService, db: LmDB) -> Self {
        Self {
            entry_service: EntryService::new(db.clone()),
            opendal_service,
            db,
        }
    }

    pub fn new_from_config(config: &ConfigToml, data_directory: &Path, db: LmDB) -> Result<Self, FileIoError> {
        let user_quota_bytes = match config.general.user_storage_quota_mb {
            0 => u64::MAX,
            other => other * 1024 * 1024,
        };
        let opendal_service = OpendalService::new_from_config(&config.storage, data_directory, &db, user_quota_bytes)?;
        Ok(Self::new(opendal_service, db))
    }

    /// Get the metadata of a file.
    pub async fn get_info(&self, path: &EntryPath) -> Result<Entry, FileIoError> {
        self.db.get_entry(path)
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked.
    /// Errors if the file does not exist.
    pub async fn get_stream(&self, path: &EntryPath) -> Result<FileStream, FileIoError> {
        let stream: FileStream = self.opendal_service.get_stream(path).await?;
        Ok(stream)
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write_stream(
        &self,
        path: &EntryPath,
        stream: impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send,
    ) -> Result<Entry, FileIoError> {

        let metadata = self
            .opendal_service
            .write_stream(path, stream)
            .await?;

        let write_result = self.entry_service.write_entry(path, &metadata);
        if let Err(e) = &write_result {
            tracing::warn!(
                "Writing entry {path} failed. Undoing the write. Error: {:?}",
                e
            );
            // Writing the entry failed. Delete the file in storage and return the error.
            self.opendal_service.delete(path).await?;
        };

        write_result
    }

    /// Delete a file.
    pub async fn delete(&self, path: &EntryPath) -> Result<(), FileIoError> {
        self.entry_service.delete_entry(path)?;
        if let Err(e) = self.opendal_service.delete(path).await {
            tracing::warn!("Deleted entry but deleting opendal file {path} failed. Potential orphaned file. Error: {:?}", e);
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(test)]
impl FileService {
    pub fn new_from_context(context: &AppContext) -> Result<Self, FileIoError> {
        let opendal_service = OpendalService::new(&context)?;
        Ok(Self::new(opendal_service, context.db.clone()))
    }

    /// Get the content of a file as bytes.
    /// Errors if the file does not exist.
    pub async fn get(&self, path: &EntryPath) -> Result<Bytes, FileIoError> {
        let mut stream = self.get_stream(path).await?;
        let mut collected_data = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            collected_data.extend_from_slice(&chunk);
        }

        Ok(Bytes::from(collected_data))
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write(&self, path: &EntryPath, data: Buffer) -> Result<Entry, FileIoError> {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from(data.to_vec()))]);
        let entry = self.write_stream(path, stream).await?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use futures_lite::StreamExt;
    use crate::{persistence::files::user_quota_layer::FILE_METADATA_SIZE, shared::webdav::WebDavPath};

    use super::*;

    #[tokio::test]
    async fn test_write_get_delete_lmdb_and_opendal() {
        let context = AppContext::test();
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();
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
        let _ = file_service.write_stream(&path, stream).await.unwrap();
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64 + FILE_METADATA_SIZE),
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
        let _ = file_service.write_stream(&path, stream).await.unwrap();
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64 + FILE_METADATA_SIZE),
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
        let context = AppContext::test();
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let test_data = b"Hello, world!";
        let buffer = Buffer::from(test_data.as_slice());

        // Test LMDB
        let lmdb_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        file_service
            .write(&lmdb_path, buffer.clone())
            .await
            .unwrap();
        let content = file_service.get(&lmdb_path).await.unwrap();
        assert_eq!(content.as_ref(), test_data);

        // Test OpenDal
        let opendal_path = EntryPath::new(pubkey, WebDavPath::new("/test_opendal.txt").unwrap());
        file_service.write(&opendal_path, buffer).await.unwrap();
        let content = file_service.get(&opendal_path).await.unwrap();
        assert_eq!(content.as_ref(), test_data);
    }

    #[tokio::test]
    async fn test_data_usage_update_basic() {
        let mut context = AppContext::test();
        context.config_toml.general.user_storage_quota_mb = 1;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();
        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64 + FILE_METADATA_SIZE)
        );

        // Delete the file and check if the data usage is updated correctly.
        file_service.delete(&path).await.unwrap();
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), Some(0));
    }

    /// Override and existing entry and check if the data usage is updated correctly.
    #[tokio::test]
    async fn test_data_usage_override_existing_entry() {
        let mut context = AppContext::test();
        context.config_toml.general.user_storage_quota_mb = 1;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();

        let test_data2 = vec![2u8; 1024];
        let buffer2 = Buffer::from(test_data2.clone());
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());

        file_service.write(&path, buffer2).await.unwrap();

        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data2.len() as u64 + FILE_METADATA_SIZE)
        );
    }

    /// Write a file that is exactly at the quota and check if the data usage is updated correctly.
    #[tokio::test]
    async fn test_data_usage_exactly_to_quota() {
        let mut context = AppContext::test();
        context.config_toml.general.user_storage_quota_mb = 1;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024 * 1024 - FILE_METADATA_SIZE as usize];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();

        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64 + FILE_METADATA_SIZE)
        );
    }

    #[tokio::test]
    async fn test_data_usage_above_quota() {
        let mut context = AppContext::test();
        context.config_toml.general.user_storage_quota_mb = 1;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024 * 1024 + 1];
        let buffer = Buffer::from(test_data.clone());

        match file_service.write(&path, buffer).await {
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
        let mut context = AppContext::test();
        context.config_toml.general.user_storage_quota_mb = 1;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.db.clone();

        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();

        let test_data2 = vec![2u8; 1024 * 1024 + 1];
        let buffer2 = Buffer::from(test_data2.clone());
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());

        match file_service.write(&path, buffer2).await {
            Ok(_) => panic!("Should error for file above quota"),
            Err(FileIoError::DiskSpaceQuotaExceeded) => {} // All good
            Err(e) => {
                panic!("Should error for file above quota: {:?}", e);
            }
        }

        assert_eq!(
            db.get_user_data_usage(&pubkey).unwrap(),
            Some(test_data.len() as u64 + FILE_METADATA_SIZE)
        );
    }
}
