#[cfg(feature = "storage-gcs")]
use super::google_bucket_config::GoogleBucketConfig;

/// The storage config. Files can be either stored in a file system, in memory, or in a Google bucket
/// depending on the configuration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfigToml {
    /// Files are stored in a Google bucket.
    #[cfg(feature = "storage-gcs")]
    GoogleBucket(GoogleBucketConfig),
    /// Files are stored in memory.
    #[cfg(any(feature = "storage-memory", test))]
    InMemory,
    /// Files are stored on the local file system.
    FileSystem,
}

/// The `[storage]` TOML section: backend selection + storage-level defaults.
///
/// The `default_quota_mb` field is the preferred way to set the system-wide
/// default storage quota. When absent, the deprecated
/// `[general].user_storage_quota_mb` is used as a fallback (where `0` means
/// unlimited). When present, its value is used directly (`None` / omitted =
/// unlimited, `Some(0)` = zero storage).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StorageToml {
    /// Which backend to use (file_system, google_bucket, in_memory).
    #[serde(flatten)]
    pub backend: StorageConfigToml,

    /// Default per-user storage quota in MB.
    /// Omit for unlimited. `0` means zero storage (not unlimited).
    /// Takes precedence over the deprecated `[general].user_storage_quota_mb`.
    pub default_quota_mb: Option<u64>,
}
