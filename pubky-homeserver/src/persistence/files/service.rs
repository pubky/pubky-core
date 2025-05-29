use crate::{
    opendal_config::StorageConfigToml,
    persistence::lmdb::{tables::files::{Entry, FileLocation}, LmDB},
    shared::webdav::EntryPath,
};
use bytes::Bytes;
use futures_util::{stream::StreamExt, Stream};
use opendal::{Buffer, Operator};
use std::path::Path;

use super::{FileMetadata, FileMetadataBuilder};

/// Build the storage operator based on the config.
/// Data dir path is used to expand the data directory placeholder in the config.
pub fn build_storage_operator_from_config(
    config: &StorageConfigToml,
    data_directory: &Path,
) -> anyhow::Result<Operator> {
    let builder = match config.clone() {
        StorageConfigToml::FileSystem(mut config) => {
            config.expand_with_data_directory(&data_directory.to_path_buf());
            tracing::info!("Store files in file system: {}", config.root_directory);
            let builder = config.to_builder()?;
            opendal::Operator::new(builder)?.finish()
        }
        StorageConfigToml::GoogleBucket(config) => {
            tracing::info!(
                "Store files in a Google Cloud Storage bucket: {}",
                config.bucket_name
            );
            let builder = config.to_builder()?;
            opendal::Operator::new(builder)?.finish()
        }
        StorageConfigToml::InMemory => {
            tracing::info!("Store files in memory");
            let builder = opendal::services::Memory::default();
            opendal::Operator::new(builder)?.finish()
        }
    };
    Ok(builder)
}

/// The chunk size to use for reading and writing files.
/// This is used to avoid reading and writing the entire file at once.
/// Important: Not all opendal providers will respect this chunk size.
/// For example, Google Cloud Buckets will deliver chunks anything from 
/// 200B to 16KB but max CHUNK_SIZE.
const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct FileService {
    operator: Operator,
    db: LmDB,
}

impl FileService {
    pub fn new(operator: Operator, db: LmDB) -> Self {
        Self { operator, db }
    }

    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    pub fn new_from_config(config: &StorageConfigToml, data_directory: &Path, db: LmDB) -> Self {
        let operator = build_storage_operator_from_config(config, data_directory).unwrap();
        Self { operator, db }
    }

    /// Get the metadata of a file.
    /// Returns None if the file does not exist.
    pub async fn get_info(&self, path: &EntryPath) -> anyhow::Result<Option<Entry>> {
        self.db.get_entry(path)
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write(&self, path: &EntryPath, location: FileLocation, stream: impl Stream<Item = Result<Bytes, anyhow::Error>> + Unpin,) -> anyhow::Result<Entry> {
        let entry = match location {
            FileLocation::LMDB => {
                let metadata = self.db.write_file_from_stream(stream).await?;
                self.db.write_entry(path, &metadata, location)?
            }
            FileLocation::OpenDal => {
                let metadata = self.write_content_stream(path, stream).await?;
                self.db.write_entry(path, &metadata, location)?
            }
        };
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
                return self.db.delete_entry(path).await
            }
            FileLocation::OpenDal => {
                self.db.delete_entry(path).await?;
                self.operator.delete(path.as_str()).await?
            }
        }
        let deleted = self.db.delete_entry(path).await?;
        if !deleted {
            // File not found.
            return Ok(false);
        }
        self.operator.delete(path.as_str()).await?;
        Ok(deleted)
    }

    /// Write the content of a file to the storage.
    /// This is useful for small files or when you want to avoid the overhead of streaming.
    /// Use streamed writes for large files.
    pub async fn write_content(
        &self,
        path: &EntryPath,
        buffer: impl Into<Buffer>,
    ) -> anyhow::Result<FileMetadata> {
        let buffer: Buffer = buffer.into();
        let bytes = Bytes::from(buffer.to_vec());
        
        // Create a single-item stream from the buffer
        let stream = Box::pin(futures_util::stream::once(async move { Ok(bytes) }));
        
        // Use the existing streaming implementation
        self.write_content_stream(path, stream).await
    }

    pub async fn write_content_stream(
        &self,
        path: &EntryPath,
        mut stream: impl Stream<Item = Result<Bytes, anyhow::Error>> + Unpin,
    ) -> anyhow::Result<FileMetadata> {
        let mut writer = self.operator.writer(path.as_str()).await?;
        let mut metadata_builder = FileMetadataBuilder::default();
        // Write each chunk from the stream to the writer
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            metadata_builder.update(&chunk);
            writer.write(chunk).await?;
        }

        // Close the writer to finalize the write operation
        writer.close().await?;

        Ok(metadata_builder.finalize())
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked by the CHUNK_SIZE.
    pub async fn get_content_stream(
        &self,
        path: &EntryPath,
    ) -> anyhow::Result<impl Stream<Item = Result<Bytes, anyhow::Error>>> {
        let reader = self
            .operator
            .reader_with(path.as_str())
            .chunk(CHUNK_SIZE)
            .await?;
        let stream = reader.into_bytes_stream(0..).await?;
        // Convert the OpenDAL stream error type to anyhow::Error
        let stream = stream.map(|result| result.map_err(anyhow::Error::from));
        Ok(stream)
    }

    /// Get the content of a file as a single Bytes object.
    /// This is useful for small files or when you want to avoid the overhead of streaming.
    pub async fn get_content(&self, path: &EntryPath) -> anyhow::Result<Bytes> {
        let mut stream = self.get_content_stream(path).await?;
        let mut content = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            content.extend_from_slice(&chunk);
        }
        Ok(Bytes::from(content))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::{
        opendal_config::{FileSystemConfig, GoogleBucketConfig, GoogleServiceAccountKeyConfig},
        shared::webdav::WebDavPath,
    };
    use crate::persistence::lmdb::tables::files::{SyncInDbTempFileWriter};

    use super::*;

    fn google_bucket_config() -> Option<StorageConfigToml> {
        let service_account_path =
            Path::new("/Users/severinbuhler/git/pubky/pubky-core/pubky-stag-gcs-account.json")
                .to_path_buf();
        // Only test this if the service account path exists
        if !service_account_path.exists() {
            println!("Google Bucket config not tested because no service account file is set.");
            return None;
        }
        Some(StorageConfigToml::GoogleBucket(GoogleBucketConfig {
            bucket_name: "homeserver-test".to_string(),
            credential: GoogleServiceAccountKeyConfig::Path(service_account_path),
        }))
    }

    // Get all possible storage configs to test.
    fn get_configs() -> (Vec<StorageConfigToml>, TempDir) {
        let (fs_config, tmp_dir) = FileSystemConfig::test();
        let mut configs = vec![
            StorageConfigToml::FileSystem(fs_config),
            StorageConfigToml::InMemory,
        ];
        if let Some(google_config) = google_bucket_config() {
            configs.push(google_config);
        }
        (configs, tmp_dir)
    }

    /// Test the chunked reading of a file.
    #[tokio::test]
    async fn test_get_content_chunked() {
        let (configs, _tmp_dir) = get_configs();
        for config in configs {
            let db = LmDB::test();
            let file_service = FileService::new_from_config(&config, Path::new("/tmp/test"), db);

            let pubkey = pkarr::Keypair::random().public_key();
            let path = EntryPath::new(pubkey, WebDavPath::new("/test.txt").unwrap());

            // Write a 10KB file filled with test data
            let should_chunk_count = 5;
            let test_data = vec![42u8; should_chunk_count * CHUNK_SIZE];
            file_service
                .write_content(&path, test_data.clone())
                .await
                .unwrap();

            // Read the content back using the chunked stream
            let mut stream = file_service.get_content_stream(&path).await.unwrap();

            let mut collected_data = Vec::new();
            let mut count = 0;
            while let Some(chunk_result) = stream.next().await {
                count += 1;
                let chunk = chunk_result.unwrap();
                collected_data.extend_from_slice(&chunk);
            }

            // Verify the data matches what we wrote
            assert_eq!(
                collected_data.len(),
                test_data.len(),
                "Total size should be 10KB"
            );
            assert_eq!(
                collected_data, test_data,
                "Content should match original data"
            );

            // Verify that we received multiple chunks according to the chunk count
            assert!(count >= should_chunk_count, "Should have received x chunks");

            // Verify that the chunks are of the correct size
            assert_eq!(
                collected_data.len(),
                should_chunk_count * CHUNK_SIZE,
                "Total size should be 10KB"
            );
            assert_eq!(
                collected_data, test_data,
                "Content should match original data"
            );
        }
    }

    #[tokio::test]
    async fn test_write_content_stream() {
        let (configs, _tmp_dir) = get_configs();
        for config in configs {
            let db = LmDB::test();
            let file_service = FileService::new_from_config(&config, Path::new("/tmp/test"), db);

            let pubkey = pkarr::Keypair::random().public_key();
            let path = EntryPath::new(pubkey, WebDavPath::new("/test_stream.txt").unwrap());

            // Create test data - multiple chunks to test streaming
            let chunk_count = 3;
            let mut test_data = Vec::new();
            let mut chunks = Vec::new();

            // Create chunks with different patterns to verify order
            for i in 0..chunk_count {
                let chunk_data = vec![i as u8; CHUNK_SIZE];
                test_data.extend_from_slice(&chunk_data);
                chunks.push(Ok(Bytes::from(chunk_data)));
            }

            // Create a stream from the chunks
            let stream = futures_util::stream::iter(chunks);

            // Write the stream to storage
            file_service
                .write_content_stream(&path, stream)
                .await
                .unwrap();

            // Read the content back and verify it matches
            let read_content = file_service.get_content(&path).await.unwrap();

            assert_eq!(
                read_content.len(),
                test_data.len(),
                "Content length should match"
            );
            assert_eq!(
                read_content.to_vec(),
                test_data,
                "Content should match original data"
            );
        }
    }

    #[tokio::test]
    async fn test_delete() {
        let (configs, _tmp_dir) = get_configs();
        for config in configs {
            let mut db = LmDB::test();
            let file_service = FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone());

            let pubkey = pkarr::Keypair::random().public_key();
            let path = EntryPath::new(pubkey, WebDavPath::new("/test_delete.txt").unwrap());

            // Test deleting a non-existent file
            let deleted = file_service.delete(&path).await.unwrap();
            assert!(!deleted, "Should return false when deleting non-existent file");

            // Write a test file using the proper workflow
            let test_data = b"test data for deletion";
            // Create a temporary file and write it to the database
            let mut writer = SyncInDbTempFileWriter::new().unwrap();
            writer.write_chunk(test_data).unwrap();
            let temp_file = writer.complete().unwrap();
            
            // Write the entry to the database (this creates both metadata and file content)
            let _entry = db.write_entry_from_file(&path, &temp_file).await.unwrap();

            // Also write to the storage operator
            file_service
                .write_content(&path, test_data.as_slice())
                .await
                .unwrap();

            // Verify the file exists before deletion
            let metadata = file_service.get_info(&path).await.unwrap();
            assert!(metadata.is_some(), "File should exist before deletion");

            // Delete the file
            let deleted = file_service.delete(&path).await.unwrap();
            assert!(deleted, "Should return true when deleting existing file");

            // Verify the file no longer exists in metadata
            let metadata = file_service.get_info(&path).await.unwrap();
            assert!(metadata.is_none(), "File should not exist after deletion");

            // Verify the file content is also removed from storage
            let content_result = file_service.get_content(&path).await;
            assert!(
                content_result.is_err(),
                "Should not be able to read deleted file content"
            );

            // Test deleting the same file again
            let deleted_again = file_service.delete(&path).await.unwrap();
            assert!(
                !deleted_again,
                "Should return false when deleting already deleted file"
            );
        }
    }
}
