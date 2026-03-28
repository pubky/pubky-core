//!
//! Module to initialize tracing logs.
//!
//! This module is used to initialize tracing logs based on the values defined in the config file.
//! If the config file is not found, it will use default values.
//!
//! This method is used to initialize tracing before the homeserver is started.
//! This way, we don't miss any logs, for example config file loading errors.
//!

use crate::{ConfigToml, HomeserverPaths, SetupSource};
use tracing_subscriber::EnvFilter;

/// Initialize tracing from a [`HomeserverPaths`].
///
/// Reads (or creates) the config file via the `SetupSource` trait, then
/// delegates to [`init_from_config`].  Falls back to
/// `ConfigToml::default()` when the config cannot be read.
pub fn init_tracing(homeserver_paths: &HomeserverPaths) -> anyhow::Result<()> {
    let config = match homeserver_paths.read_or_create_config_file() {
        Ok(config) => config,
        Err(e) => {
            println!("Failed to read config from file: {}", e);
            ConfigToml::default()
        }
    };

    init_from_config_if_set(&config)
}

/// Initialize tracing logger based on the values defined in a config.
///
/// This is `pub(crate)` so that [`crate::HomeserverApp::start`] can call
/// it with an already-loaded `ConfigToml` without re-reading the file.
pub(crate) fn init_from_config_if_set(config: &ConfigToml) -> anyhow::Result<()> {
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
