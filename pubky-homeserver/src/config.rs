//! Configuration for the server

use anyhow::{anyhow, Result};
use pkarr::{mainline::dht::DhtSettings, Keypair};
// use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf, time::Duration};

use pubky_common::timestamp::Timestamp;

const DEFAULT_HOMESERVER_PORT: u16 = 6287;
const DEFAULT_STORAGE_DIR: &str = "pubky";

/// Server configuration
#[derive(
    // Serialize, Deserialize,
    Clone,
)]
pub struct Config {
    port: Option<u16>,
    bootstrap: Option<Vec<String>>,
    domain: String,
    /// Path to the storage directory
    ///
    /// Defaults to a directory in the OS data directory
    storage: Option<PathBuf>,
    keypair: Keypair,

    dht_request_timeout: Option<Duration>,
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

    /// Testnet configurations
    pub fn testnet() -> Self {
        let testnet = pkarr::mainline::Testnet::new(10);

        let bootstrap = Some(testnet.bootstrap.to_owned());
        let storage = Some(
            std::env::temp_dir()
                .join(Timestamp::now().to_string())
                .join(DEFAULT_STORAGE_DIR),
        );

        Self {
            bootstrap,
            storage,
            port: Some(15411),
            keypair: Keypair::from_secret_key(&[0_u8; 32]),
            dht_request_timeout: Some(Duration::from_millis(10)),
            ..Default::default()
        }
    }

    /// Test configurations
    pub fn test(testnet: &pkarr::mainline::Testnet) -> Self {
        let bootstrap = Some(testnet.bootstrap.to_owned());
        let storage = Some(
            std::env::temp_dir()
                .join(Timestamp::now().to_string())
                .join(DEFAULT_STORAGE_DIR),
        );

        Self {
            bootstrap,
            storage,
            ..Default::default()
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

    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    pub(crate) fn dht_request_timeout(&self) -> Option<Duration> {
        self.dht_request_timeout
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: Some(0),
            bootstrap: None,
            domain: "localhost".to_string(),
            storage: None,
            keypair: Keypair::random(),
            dht_request_timeout: None,
        }
    }
}

impl Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entry(&"port", &self.port())
            .entry(&"storage", &self.storage())
            .entry(&"public_key", &self.keypair().public_key())
            .finish()
    }
}
