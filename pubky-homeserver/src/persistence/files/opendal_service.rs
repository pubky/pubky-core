use std::path::Path;

#[cfg(test)]
use crate::AppContext;
use crate::{
    persistence::{files::user_quota_layer::UserQuotaLayer, lmdb::LmDB},
    shared::webdav::EntryPath,
    storage_config::StorageConfigToml,
};
use bytes::Bytes;
use futures_util::{stream::StreamExt, Stream};
#[cfg(test)]
use opendal::Buffer;
use opendal::Operator;

use super::{FileIoError, FileMetadata, FileMetadataBuilder, FileStream, WriteStreamError};

/// Build the storage operator based on the config.
/// Data dir path is used to expand the data directory placeholder in the config.
pub fn build_storage_operator(
    storage_config: &StorageConfigToml,
    data_directory: &Path,
    db: &LmDB,
    user_quota_bytes: u64,
) -> Result<Operator, FileIoError> {
    let user_quota_layer = UserQuotaLayer::new(db.clone(), user_quota_bytes);
    let builder = match storage_config {
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
            opendal::Operator::new(builder)?
                .layer(user_quota_layer)
                .finish()
        }
        #[cfg(feature = "storage-gcs")]
        StorageConfigToml::GoogleBucket(config) => {
            tracing::info!(
                "Store files in a Google Cloud Storage bucket: {}",
                config.bucket_name
            );
            let builder = config.to_builder()?;
            opendal::Operator::new(builder)?
                .layer(user_quota_layer)
                .finish()
        }
        #[cfg(any(feature = "storage-memory", test))]
        StorageConfigToml::InMemory => {
            tracing::info!("Store files in memory");
            let builder = opendal::services::Memory::default();
            opendal::Operator::new(builder)?
                .layer(user_quota_layer)
                .finish()
        }
    };
    Ok(builder)
}

/// Build the storage operator based on the config.
/// Data dir path is used to expand the data directory placeholder in the config.
#[cfg(test)]
pub fn build_storage_operator_from_context(context: &AppContext) -> Result<Operator, FileIoError> {
    let quota_bytes = match context.config_toml.general.user_storage_quota_mb {
        0 => u64::MAX,
        other => other * 1024 * 1024,
    };
    build_storage_operator(
        &context.config_toml.storage,
        context.data_dir.path(),
        &context.db,
        quota_bytes,
    )
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
    pub(crate) operator: Operator,
}

impl OpendalService {
    pub fn new_from_config(
        config: &StorageConfigToml,
        data_directory: &Path,
        db: &LmDB,
        user_quota_bytes: u64,
    ) -> Result<Self, FileIoError> {
        let operator = build_storage_operator(config, data_directory, db, user_quota_bytes)?;
        Ok(Self { operator })
    }

    /// Delete a file.
    /// Deleting a non-existing file will NOT return an error.
    pub async fn delete(&self, path: &EntryPath) -> Result<(), FileIoError> {
        self.operator
            .delete(path.as_str())
            .await
            .map_err(FileIoError::OpenDAL)
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
    ) -> Result<FileMetadata, FileIoError> {
        let mut writer = self.operator.writer(path.as_str()).await?;
        let mut metadata_builder = FileMetadataBuilder::default();
        metadata_builder.guess_mime_type_from_path(path.path().as_str());

        // Write each chunk from the stream to the writer
        let write_result: Result<(), FileIoError> = async {
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
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
                writer.close().await.map_err(|e| {
                    // The UserQuotaLayer will return a RateLimited error if the user has exceeded the quota.
                    // We convert this to a DiskSpaceQuotaExceeded error.
                    if e.kind() == opendal::ErrorKind::RateLimited
                        && e.to_string().contains("User quota exceeded")
                    {
                        FileIoError::DiskSpaceQuotaExceeded
                    } else {
                        FileIoError::OpenDAL(e)
                    }
                })?;
                Ok(metadata_builder.finalize())
            }
            Err(e) => {
                // Abort the writer properly to avoid leaking resources.
                writer.abort().await?;
                Err(e)
            }
        }
    }

    /// Get the stream of a file.
    /// Helper method because the NOT_FOUND error can happen in two different places.
    async fn get_stream_inner(&self, path: &EntryPath) -> Result<FileStream, opendal::Error> {
        let reader = self
            .operator
            .reader_with(path.as_str())
            .chunk(CHUNK_SIZE)
            .await?;

        let stream = reader.into_bytes_stream(0..).await?;
        Ok(Box::new(stream))
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked by the CHUNK_SIZE.
    pub async fn get_stream(&self, path: &EntryPath) -> Result<FileStream, FileIoError> {
        match self.get_stream_inner(path).await {
            Ok(stream) => Ok(stream),
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
}

#[cfg(test)]
impl OpendalService {
    pub fn new(context: &AppContext) -> Result<Self, FileIoError> {
        let operator = build_storage_operator_from_context(context)?;
        Ok(Self { operator })
    }

    /// Create a new opendal service from an existing operator.
    /// This is useful for testing.
    pub fn new_from_operator(operator: Operator) -> Self {
        Self { operator }
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
        self.write_stream(path, stream).await
    }

    /// Check if a file exists.
    #[cfg(test)]
    pub async fn exists(&self, path: &EntryPath) -> Result<bool, opendal::Error> {
        self.operator.exists(path.as_str()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::files::opendal_test_operators::OpendalTestOperators;
    use crate::shared::webdav::WebDavPath;

    #[tokio::test]
    async fn test_build_storage_operator_from_config_file_system() {
        let mut context = AppContext::test();
        context.config_toml.storage = StorageConfigToml::FileSystem;

        let service =
            OpendalService::new(&context).expect("Failed to create OpenDAL service for testing");
        let pubky = pkarr::Keypair::random().public_key();
        context
            .db
            .create_user(&pubky)
            .expect("Failed to create user");
        let path = EntryPath::new(pubky, WebDavPath::new("/test.txt").unwrap());
        assert!(!service.exists(&path).await.unwrap());
    }

    /// Make sure that the OpendalService returns a DiskSpaceQuotaExceeded error if the user has exceeded the quota.
    /// This is important because the UserQuotaLayer will return a RateLimited error if the user has exceeded the quota.
    #[tokio::test]
    async fn test_quota_exceeded_error() {
        let mut context = AppContext::test();
        context.config_toml.general.user_storage_quota_mb = 1;
        let service =
            OpendalService::new(&context).expect("Failed to create OpenDAL service for testing");
        let pubky = pkarr::Keypair::random().public_key();
        context
            .db
            .create_user(&pubky)
            .expect("Failed to create user");
        let path = EntryPath::new(pubky, WebDavPath::new("/test.txt").unwrap());
        let write_result = service.write(&path, vec![42u8; 1024 * 1024]).await;
        assert!(write_result.is_err());
        assert!(matches!(
            write_result,
            Err(FileIoError::DiskSpaceQuotaExceeded)
        ));
    }

    /// Test the chunked reading of a file.
    #[tokio::test]
    async fn test_get_content_chunked() {
        let operators = OpendalTestOperators::new();
        for (_scheme, operator) in operators.operators() {
            let file_service = OpendalService::new_from_operator(operator);

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
        let operators = OpendalTestOperators::new();
        for (_scheme, operator) in operators.operators() {
            let file_service = OpendalService::new_from_operator(operator);

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
            file_service.write_stream(&path, stream).await.unwrap();

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
