#[cfg(feature = "storage-gcs")]
mod google_bucket_config;
mod storage_config_toml;

#[cfg(feature = "storage-gcs")]
pub use google_bucket_config::{GoogleBucketConfig, GoogleServiceAccountKeyConfig};

pub use storage_config_toml::StorageConfigToml;
