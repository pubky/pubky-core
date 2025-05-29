use opendal::{Buffer, Operator};
use crate::{opendal_config::StorageConfigToml, persistence::lmdb::{tables::files::Entry, LmDB}, shared::webdav::EntryPath};
use std::path::Path;
use futures_util::{stream::StreamExt, Stream};
use bytes::Bytes;

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
/// For example, Google Cloud Buckets will deliver chunks anything from 200B to 16KB. 
const CHUNK_SIZE: usize = 64*1024;

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

    pub async fn get_metadata(&self, path: &EntryPath) -> anyhow::Result<Option<Entry>> {
        self.db.get_entry(path)
    }

    /// Write the content of a file to the storage.
    /// This is useful for small files or when you want to avoid the overhead of streaming.
    /// Use streamed writes for large files.
    pub async fn write_content(&self, path: &EntryPath, buffer: impl Into<Buffer>) -> anyhow::Result<()> {
        self.operator.write(path.as_str(), buffer).await?;
        Ok(())
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked by the CHUNK_SIZE.
    pub async fn get_content_stream(&self, path: &EntryPath) -> anyhow::Result<impl Stream<Item = Result<Bytes, anyhow::Error>>> {
        let reader = self.operator.reader_with(path.as_str()).chunk(CHUNK_SIZE).await?;
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
    use opendal::Buffer;
    use tempfile::TempDir;

    use crate::{opendal_config::{FileSystemConfig, GoogleBucketConfig, GoogleServiceAccountKeyConfig}, shared::webdav::WebDavPath};

    use super::*;

    fn file_system_config() -> (TempDir, StorageConfigToml) {
        let tmp_dir = tempfile::tempdir().unwrap();
        let config = StorageConfigToml::FileSystem(FileSystemConfig{
            root_directory: tmp_dir.path().to_string_lossy().to_string(),
        });
        (tmp_dir, config)
    }

    fn google_bucket_config() -> Option<StorageConfigToml> {
        let service_account_path = Path::new("/Users/severinbuhler/git/pubky/pubky-core/pubky-stag-gcs-account.json").to_path_buf();
        // Only test this if the service account path exists
        if !service_account_path.exists() {
            println!("Google Bucket config not tested because no service account file is set.");
            return None;
        }
        Some(StorageConfigToml::GoogleBucket(GoogleBucketConfig{
            bucket_name: "homeserver-test".to_string(),
            credential: GoogleServiceAccountKeyConfig::Path(service_account_path),
        }))
    }

    // Get all possible storage configs to test.
    fn get_configs() -> (Vec<StorageConfigToml>, TempDir) {
        let (tmp_dir, fs_config) = file_system_config();
        let mut configs = vec![
            fs_config, 
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
            file_service.write_content(&path, test_data.clone()).await.unwrap();

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
            assert_eq!(collected_data.len(), test_data.len(), "Total size should be 10KB");
            assert_eq!(collected_data, test_data, "Content should match original data");
            
            // Verify that we received multiple chunks according to the chunk count
            assert!(count >= should_chunk_count, "Should have received x chunks");

            // Verify that the chunks are of the correct size
            assert_eq!(collected_data.len(), should_chunk_count * CHUNK_SIZE, "Total size should be 10KB");
            assert_eq!(collected_data, test_data, "Content should match original data");
        }
    }
}