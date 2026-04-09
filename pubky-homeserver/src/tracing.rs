//!
//! Module to initialize tracing logs.
//!
//! This module is used to initialize tracing logs based on the values defined in the config file.
//! If the config file is not found, it will use default values.
//!
//! This method is used to initialize tracing before the homeserver is started.
//! This way, we don't miss any logs, for example config file loading errors.
//!

use crate::{ConfigToml, PersistentDataDir};
use std::path::Path;
use tracing_subscriber::EnvFilter;

fn read_config_from_file(data_dir: &Path) -> anyhow::Result<ConfigToml> {
    let data_dir = PersistentDataDir::new(data_dir.to_path_buf());
    let config_file_path = data_dir.get_config_file_path();
    let config = ConfigToml::from_file(config_file_path)?;
    Ok(config)
}

/// Initialize tracing logger based on the values defined in the config file.
pub fn init_tracing_logs_with_config_if_set(config: &ConfigToml) -> anyhow::Result<()> {
    let config = match &config.logging {
        Some(config) => config,
        None => return Ok(()),
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let mut filter = EnvFilter::new("");
        filter = filter.add_directive(config.level.to_owned().into());
        // Add any specific filters
        for filter_str in &config.module_levels {
            filter = filter.add_directive(filter_str.to_owned().into());
        }
        filter
    });
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize tracing: {}", e))?;

    Ok(())
}

/// Initialize tracing logger based on the values defined in the config file.
/// If the config file is not found, use default values.
pub fn init_tracing_logs_if_set(data_dir: &Path) -> anyhow::Result<()> {
    let config = match read_config_from_file(data_dir) {
        Ok(config) => config,
        Err(e) => {
            println!("Failed to read config from file: {}", e);
            ConfigToml::default()
        }
    };

    init_tracing_logs_with_config_if_set(&config)
}
