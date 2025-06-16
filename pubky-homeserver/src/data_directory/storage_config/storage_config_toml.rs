use super::google_bucket_config::GoogleBucketConfig;

/// The storage config. Files can be either stored in a file system, in memory, or in a Google bucket
/// depending on the configuration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfigToml {
    /// Files are stored in a Google bucket.
    GoogleBucket(GoogleBucketConfig),
    /// Files are stored in memory.
    InMemory,
    /// Files are stored on the local file system.
    FileSystem,
}
