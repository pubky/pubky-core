use crate::{
    persistence::lmdb::{
        tables::files::{Entry, FileLocation},
        LmDB,
    },
    shared::webdav::EntryPath,
    ConfigToml,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use opendal::Buffer;
use std::path::Path;

use super::{write_disk_quota_enforcer::WriteDiskQuotaEnforcer, OpendalService, WriteFileFromStreamError, WriteStreamError};


#[derive(Debug, thiserror::Error)]
pub enum FileServiceWriteError {
    #[error("Disk space quota exceeded")]
    DiskSpaceQuotaExceeded,
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// The file service creates an abstraction layer over the LMDB and OpenDAL services.
/// This way, files can be managed in a unified way.
#[derive(Debug, Clone)]
pub struct FileService {
    opendal_service: OpendalService,
    db: LmDB,
    user_quota_bytes: Option<u64>,
}

impl FileService {
    pub fn new(opendal_service: OpendalService, db: LmDB, user_quota_bytes: Option<u64>) -> Self {
        Self {
            opendal_service,
            db,
            user_quota_bytes,
        }
    }

    pub fn new_from_config(config: &ConfigToml, data_directory: &Path, db: LmDB) -> Self {
        let opendal_service = OpendalService::new_from_config(&config.storage, data_directory);
        let quota_mb = config.general.user_storage_quota_mb;
        let quota_bytes = if quota_mb == 0 {
            None
        } else {
            Some(quota_mb * 1024 * 1024)
        };
        Self::new(opendal_service, db, quota_bytes)
    }

    /// Get the metadata of a file.
    /// Returns None if the file does not exist.
    pub async fn get_info(&self, path: &EntryPath) -> anyhow::Result<Option<Entry>> {
        self.db.get_entry(path)
    }

    /// Get the content of a file as bytes.
    /// Errors if the file does not exist.
    pub async fn get(&self, path: &EntryPath) -> anyhow::Result<Bytes> {
        let mut stream = self.get_stream(path).await?;
        let mut collected_data = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| anyhow::anyhow!("Error reading chunk: {}", e))?;
            collected_data.extend_from_slice(&chunk);
        }

        Ok(Bytes::from(collected_data))
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked.
    /// Errors if the file does not exist.
    pub async fn get_stream(
        &self,
        path: &EntryPath,
    ) -> anyhow::Result<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Unpin + Send>> {
        let entry = match self.get_info(path).await? {
            Some(entry) => entry,
            None => anyhow::bail!("File not found"),
        };
        let stream = match entry.file_location() {
            FileLocation::LMDB => {
                let temp_file = self.db.read_file(&entry.file_id()).await?;
                Box::new(temp_file.as_stream()?)
                    as Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Unpin + Send>
            }
            FileLocation::OpenDal => Box::new(self.opendal_service.get_stream(path).await?)
                as Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Unpin + Send>,
        };
        Ok(stream)
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write(
        &self,
        path: &EntryPath,
        data: Buffer,
        location: FileLocation,
    ) -> Result<Entry, WriteFileFromStreamError> {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from(data.to_vec()))]);
        self.write_stream(path, location, stream).await
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write_stream(
        &self,
        path: &EntryPath,
        location: FileLocation,
        stream: impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send,
    ) -> Result<Entry, WriteFileFromStreamError> {

        let mut stream: Box<dyn Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send> =
            Box::new(stream);
        if let Some(user_quota_bytes) = self.user_quota_bytes {
            // Enforce disk size quota on the stream
            stream = Box::new(WriteDiskQuotaEnforcer::new(
                stream,
                &self.db,
                path,
                user_quota_bytes,
            )?);
        }

        let existing_entry_bytes = self.db.get_entry_content_length(path)?;

        let entry = match location {
            FileLocation::LMDB => {
                let metadata = self.db.write_file_from_stream(stream).await?;
                self.db.write_entry(path, &metadata, location)?
            }
            FileLocation::OpenDal => {
                let metadata = self.opendal_service.write_stream(path, stream).await?;
                self.db.write_entry(path, &metadata, location)?
            }
        };
        // Update usage counter based on the actual file size
        let delta = entry.content_length() as i64 - existing_entry_bytes as i64;
        self.db.update_data_usage(path.pubkey(), delta)?;
        Ok(entry)
    }

    /// Delete a file.
    pub async fn delete(&self, path: &EntryPath) -> anyhow::Result<bool> {
        let entry = match self.get_info(path).await? {
            Some(entry) => entry,
            None => return Ok(false),
        };
        match entry.file_location() {
            FileLocation::LMDB => {
                self.db.delete_entry_and_file(path).await?;
            },
            FileLocation::OpenDal => {
                self.db.delete_entry(path).await?;
                self.opendal_service.delete(path).await?;
            }
        };
        self.db.update_data_usage(path.pubkey(), -(entry.content_length() as i64))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use futures_lite::StreamExt;

    use crate::{opendal_config::StorageConfigToml, shared::webdav::WebDavPath};

    use super::*;

    #[tokio::test]
    async fn test_write_get_delete_lmdb_and_opendal() {
        let mut config = ConfigToml::test();
        config.storage = StorageConfigToml::InMemory;
        let db = LmDB::test();
        let file_service = FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone());

        let pubkey = pkarr::Keypair::random().public_key();
        let mut wtxn = db.env.write_txn().unwrap();
        db.create_user(&pubkey, &mut wtxn).unwrap();
        wtxn.commit().unwrap();

        // User should not have any data usage yet
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), 0);

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());

        // Test getting a non-existent file
        let result = file_service.get_stream(&path).await;
        assert!(result.is_err(), "Should error for non-existent file");

        // Test data
        let test_data = b"Hello, world! This is test data for the get method.";
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);

        // Test LMDB
        let entry = file_service
            .write_stream(&path, FileLocation::LMDB, stream)
            .await
            .unwrap();
        assert_eq!(*entry.file_location(), FileLocation::LMDB);
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), test_data.len() as u64, "Data usage should be the size of the file");

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
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), 0, "Data usage should be 0 after deleting file");

        // Test OpenDal location
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_opendal.txt").unwrap());
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);
        let entry = file_service
            .write_stream(&path, FileLocation::OpenDal, stream)
            .await
            .unwrap();
        assert_eq!(*entry.file_location(), FileLocation::OpenDal);
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), test_data.len() as u64, "Data usage should be the size of the file");

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
        assert_eq!(db.get_user_data_usage(&pubkey).unwrap(), 0, "Data usage should be 0 after deleting file");
    }

    #[tokio::test]
    async fn test_write_get_basic() {
        let mut config = ConfigToml::test();
        config.storage = StorageConfigToml::InMemory;
        let db = LmDB::test();
        let file_service = FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone());

        let pubkey = pkarr::Keypair::random().public_key();
        let mut wtxn = db.env.write_txn().unwrap();
        db.create_user(&pubkey, &mut wtxn).unwrap();
        wtxn.commit().unwrap();

        let test_data = b"Hello, world!";
        let buffer = Buffer::from(test_data.as_slice());

        // Test LMDB
        let lmdb_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_lmdb.txt").unwrap());
        file_service
            .write(&lmdb_path, buffer.clone(), FileLocation::LMDB)
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
}
