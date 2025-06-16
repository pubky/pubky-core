use crate::{shared::webdav::EntryPath, storage_config::StorageConfigToml};
use bytes::Bytes;
use futures_util::{stream::StreamExt, Stream};
#[cfg(test)]
use opendal::Buffer;
use opendal::Operator;
use std::path::Path;

use super::{FileIoError, FileMetadata, FileMetadataBuilder, FileStream, WriteStreamError};

/// Build the storage operator based on the config.
/// Data dir path is used to expand the data directory placeholder in the config.
pub fn build_storage_operator_from_config(
    config: &StorageConfigToml,
    data_directory: &Path,
) -> Result<Operator, FileIoError> {
    let builder = match config.clone() {
        StorageConfigToml::FileSystem => {
            let files_dir = match data_directory.join("data/files").to_str() {
                Some(path) => path.to_string(),
                None => {
                    return Err(FileIoError::OpenDAL(opendal::Error::new(
                        opendal::ErrorKind::Unexpected,
                        "Invalid path",
                    )))
                }
            };

            let builder = opendal::services::Fs::default().root(files_dir.as_str());
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
const CHUNK_SIZE: usize = 16 * 1024;

/// The service to write and read files to and from the configured opendal storage.
#[derive(Debug, Clone)]
pub struct OpendalService {
    operator: Operator,
}

impl OpendalService {
    pub fn new_from_config(
        config: &StorageConfigToml,
        data_directory: &Path,
    ) -> Result<Self, FileIoError> {
        let operator = build_storage_operator_from_config(config, data_directory)?;
        Ok(Self { operator })
    }

    /// Delete a file.
    pub async fn delete(&self, path: &EntryPath) -> Result<(), FileIoError> {
        self.operator
            .delete(path.as_str())
            .await
            .map_err(FileIoError::OpenDAL)
    }

    /// Write the content of a file to the storage.
    /// This is useful for small files or when you want to avoid the overhead of streaming.
    /// Use streamed writes for large files.
    #[cfg(test)]
    pub async fn write(
        &self,
        path: &EntryPath,
        buffer: impl Into<Buffer>,
    ) -> Result<FileMetadata, FileIoError> {
        let buffer: Buffer = buffer.into();
        let bytes = Bytes::from(buffer.to_vec());
        // Create a single-item stream from the buffer
        let stream = Box::pin(futures_util::stream::once(async move { Ok(bytes) }));
        // Use the existing streaming implementation
        self.write_stream(path, stream, None).await
    }

    /// Write a stream to the storage.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file.
    /// * `stream` - The stream to write.
    /// * `max_bytes` - The maximum number of bytes to write. Will throw an error if the stream exceeds this limit.
    pub async fn write_stream(
        &self,
        path: &EntryPath,
        mut stream: impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send,
        max_bytes: Option<u64>,
    ) -> Result<FileMetadata, FileIoError> {
        let mut writer = self.operator.writer(path.as_str()).await?;
        let mut metadata_builder = FileMetadataBuilder::default();
        metadata_builder.guess_mime_type_from_path(path.path().as_str());

        // Write each chunk from the stream to the writer
        let write_result: Result<(), FileIoError> = async {
            let mut bytes_counter = 0;
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                bytes_counter += chunk.len() as u64;
                if let Some(max_bytes) = max_bytes {
                    if bytes_counter > max_bytes {
                        return Err(FileIoError::DiskSpaceQuotaExceeded);
                    }
                }
                metadata_builder.update(&chunk);
                writer.write(chunk).await?;
            }
            Ok(())
        }
        .await;

        // Let's close the writer properly depending on if the stream write was successful.
        match write_result {
            Ok(()) => {
                // Close the writer to finalize the write operation
                writer.close().await?;
                Ok(metadata_builder.finalize())
            }
            Err(e) => {
                // Abort the writer properly to avoid leaking resources.
                writer.abort().await?;
                Err(e)
            }
        }
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked by the CHUNK_SIZE.
    pub async fn get_stream(&self, path: &EntryPath) -> Result<FileStream, FileIoError> {
        match self
            .operator
            .reader_with(path.as_str())
            .chunk(CHUNK_SIZE)
            .await
        {
            Ok(reader) => Ok(Box::new(reader.into_bytes_stream(0..).await?)),
            Err(e) => match e.kind() {
                opendal::ErrorKind::NotFound => Err(FileIoError::NotFound),
                opendal::ErrorKind::PermissionDenied => {
                    tracing::warn!(
                        "Permission denied for path: {}. Treating as not found.",
                        path
                    );
                    Err(FileIoError::NotFound)
                }
                _ => {
                    tracing::error!("OpenDAL error for path {}: {}", path, e);
                    Err(FileIoError::OpenDAL(e))
                }
            },
        }
    }

    /// Get the content of a file as a single Bytes object.
    /// This is useful for small files or when you want to avoid the overhead of streaming.
    #[cfg(test)]
    pub async fn get(&self, path: &EntryPath) -> Result<Bytes, FileIoError> {
        let mut stream = self.get_stream(path).await?;
        let mut content = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            content.extend_from_slice(&chunk);
        }
        Ok(Bytes::from(content))
    }

    /// Check if a file exists.
    #[cfg(test)]
    pub async fn exists(&self, path: &EntryPath) -> Result<bool, opendal::Error> {
        self.operator.exists(path.as_str()).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        shared::webdav::WebDavPath,
        storage_config::{GoogleBucketConfig, GoogleServiceAccountKeyConfig},
    };

    use super::*;

    fn google_bucket_config() -> Option<StorageConfigToml> {
        let service_account_path = Path::new("../gcs-service-account.json").to_path_buf();
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
    fn get_configs() -> Vec<StorageConfigToml> {
        let mut configs = vec![StorageConfigToml::FileSystem, StorageConfigToml::InMemory];
        if let Some(google_config) = google_bucket_config() {
            configs.push(google_config);
        }
        configs
    }

    /// Test the chunked reading of a file.
    #[tokio::test]
    async fn test_get_content_chunked() {
        let configs = get_configs();
        for config in configs {
            let file_service = OpendalService::new_from_config(&config, Path::new("/tmp/test"))
                .expect("Failed to create OpenDAL service for testing");

            let pubkey = pkarr::Keypair::random().public_key();
            let path = EntryPath::new(pubkey, WebDavPath::new("/test.txt").unwrap());

            // Write a 10KB file filled with test data
            let should_chunk_count = 5;
            let test_data = vec![42u8; should_chunk_count * CHUNK_SIZE];
            file_service.write(&path, test_data.clone()).await.unwrap();

            // Read the content back using the chunked stream
            let mut stream = file_service.get_stream(&path).await.unwrap();

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

            file_service
                .delete(&path)
                .await
                .expect("Should delete file");
            assert!(
                !file_service.exists(&path).await.unwrap(),
                "File should not exist after deletion"
            );
        }
    }

    #[tokio::test]
    async fn test_write_content_stream() {
        let configs = get_configs();
        for config in configs {
            let file_service = OpendalService::new_from_config(&config, Path::new("/tmp/test"))
                .expect("Failed to create OpenDAL service for testing");

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
                .write_stream(&path, stream, None)
                .await
                .unwrap();

            // Read the content back and verify it matches
            let read_content = file_service.get(&path).await.unwrap();

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

            file_service
                .delete(&path)
                .await
                .expect("Should delete file");
            assert!(
                !file_service.exists(&path).await.unwrap(),
                "File should not exist after deletion"
            );
        }
    }
}
