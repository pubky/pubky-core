use std::path::Path;

use opendal::Operator;

use crate::opendal_config::StorageConfigToml;

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
