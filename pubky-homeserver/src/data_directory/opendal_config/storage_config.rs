use super::{file_system_config::FileSystemConfig, google_bucket_config::GoogleBucketConfig, in_memory_config::InMemoryConfig};


/// The storage config. Files can be either stored in a file system, in memory, or in a Google bucket.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfig {
    GoogleBucket(GoogleBucketConfig),
    FileSystem(FileSystemConfig),
    InMemory(InMemoryConfig),
}

