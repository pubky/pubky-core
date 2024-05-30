//! Configuration for the server

use anyhow::{anyhow, Result};
// use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf};

const DEFAULT_HOMESERVER_PORT: u16 = 6287;
const DEFAULT_STORAGE_DIR: &str = "pubky";

/// Server configuration
///
/// The config is usually loaded from a file with [`Self::load`].
#[derive(
    // Serialize, Deserialize,
    Default,
)]
pub struct Config {
    port: Option<u16>,
    bootstrap: Option<Vec<String>>,
    domain: String,
    /// Path to the storage directory
    ///
    /// Defaults to a directory in the OS data directory
    storage: Option<PathBuf>,
}

impl Config {
    // /// Load the config from a file.
    // pub async fn load(path: impl AsRef<Path>) -> Result<Config> {
    //     let s = tokio::fs::read_to_string(path.as_ref())
    //         .await
    //         .with_context(|| format!("failed to read {}", path.as_ref().to_string_lossy()))?;
    //     let config: Config = toml::from_str(&s)?;
    //     Ok(config)
    // }

    /// Test configurations
    pub fn test(testnet: &pkarr::mainline::Testnet) -> Self {
        Self {
            port: Some(0),
            bootstrap: Some(testnet.bootstrap.to_owned()),
            domain: "localhost".to_string(),
            storage: Some(
                std::env::temp_dir()
                    .join(pk_common::time::Timestamp::now().to_string())
                    .join(DEFAULT_STORAGE_DIR),
            ),
        }
    }

    pub fn port(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_HOMESERVER_PORT)
    }

    pub fn bootstsrap(&self) -> Option<Vec<String>> {
        self.bootstrap.to_owned()
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Get the path to the storage directory
    pub fn storage(&self) -> Result<PathBuf> {
        let dir = if let Some(storage) = &self.storage {
            PathBuf::from(storage)
        } else {
            let path = dirs_next::data_dir().ok_or_else(|| {
                anyhow!("operating environment provides no directory for application data")
            })?;
            path.join(DEFAULT_STORAGE_DIR)
        };

        Ok(dir.join("homeserver"))
    }
}

impl Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"port", &self.port())
            .entry(&"storage", &self.storage())
            .finish()
    }
}
