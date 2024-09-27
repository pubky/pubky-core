//! Configuration for the server

use anyhow::{anyhow, Context, Result};
use pkarr::Keypair;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::info;

use pubky_common::timestamp::Timestamp;

const DEFAULT_HOMESERVER_PORT: u16 = 6287;
const DEFAULT_STORAGE_DIR: &str = "pubky";

pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct ConfigToml {
    testnet: Option<bool>,
    port: Option<u16>,
    bootstrap: Option<Vec<String>>,
    domain: Option<String>,
    storage: Option<PathBuf>,
    secret_key: Option<String>,
    dht_request_timeout: Option<Duration>,
    default_list_limit: Option<u16>,
    max_list_limit: Option<u16>,
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether or not this server is running in a testnet.
    testnet: bool,
    /// The configured port for this server.
    port: Option<u16>,
    /// Bootstrapping DHT nodes.
    ///
    /// Helpful to run the server locally or in testnet.
    bootstrap: Option<Vec<String>>,
    /// A public domain for this server
    /// necessary for web browsers running in https environment.
    domain: Option<String>,
    /// Path to the storage directory.
    ///
    /// Defaults to a directory in the OS data directory
    storage: PathBuf,
    /// Server keypair.
    ///
    /// Defaults to a random keypair.
    keypair: Keypair,
    dht_request_timeout: Option<Duration>,
    /// The default limit of a list api if no `limit` query parameter is provided.
    ///
    /// Defaults to `100`
    default_list_limit: u16,
    /// The maximum limit of a list api, even if a `limit` query parameter is provided.
    ///
    /// Defaults to `1000`
    max_list_limit: u16,
}

impl Config {
    fn try_from_str(value: &str) -> Result<Self> {
        let config_toml: ConfigToml = toml::from_str(value)?;

        let keypair = if let Some(secret_key) = config_toml.secret_key {
            let secret_key = deserialize_secret_key(secret_key)?;
            Keypair::from_secret_key(&secret_key)
        } else {
            Keypair::random()
        };

        let storage = {
            let dir = if let Some(storage) = config_toml.storage {
                storage
            } else {
                let path = dirs_next::data_dir().ok_or_else(|| {
                    anyhow!("operating environment provides no directory for application data")
                })?;
                path.join(DEFAULT_STORAGE_DIR)
            };

            dir.join("homeserver")
        };

        let config = Config {
            testnet: config_toml.testnet.unwrap_or(false),
            port: config_toml.port,
            bootstrap: config_toml.bootstrap,
            domain: config_toml.domain,
            keypair,
            storage,
            dht_request_timeout: config_toml.dht_request_timeout,
            default_list_limit: config_toml.default_list_limit.unwrap_or(DEFAULT_LIST_LIMIT),
            max_list_limit: config_toml
                .default_list_limit
                .unwrap_or(DEFAULT_MAX_LIST_LIMIT),
        };

        if config.testnet {
            let testnet_config = Config::testnet();

            return Ok(Config {
                bootstrap: testnet_config.bootstrap,
                ..config
            });
        }

        Ok(config)
    }

    /// Load the config from a file.
    pub async fn load(path: impl AsRef<Path>) -> Result<Config> {
        let s = tokio::fs::read_to_string(path.as_ref())
            .await
            .with_context(|| format!("failed to read {}", path.as_ref().to_string_lossy()))?;

        Config::try_from_str(&s)
    }

    /// Testnet configurations
    pub fn testnet() -> Self {
        let testnet = pkarr::mainline::Testnet::new(10);
        info!(?testnet.bootstrap, "Testnet bootstrap nodes");

        let bootstrap = Some(testnet.bootstrap.to_owned());
        let storage = std::env::temp_dir()
            .join(Timestamp::now().to_string())
            .join(DEFAULT_STORAGE_DIR);

        Self {
            bootstrap,
            storage,
            port: Some(15411),
            dht_request_timeout: Some(Duration::from_millis(10)),
            ..Default::default()
        }
    }

    /// Test configurations
    pub fn test(testnet: &pkarr::mainline::Testnet) -> Self {
        let bootstrap = Some(testnet.bootstrap.to_owned());
        let storage = std::env::temp_dir()
            .join(Timestamp::now().to_string())
            .join(DEFAULT_STORAGE_DIR);

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

    pub fn domain(&self) -> &Option<String> {
        &self.domain
    }

    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    pub fn default_list_limit(&self) -> u16 {
        self.default_list_limit
    }

    pub fn max_list_limit(&self) -> u16 {
        self.max_list_limit
    }

    /// Get the path to the storage directory
    pub fn storage(&self) -> &PathBuf {
        &self.storage
    }

    pub(crate) fn dht_request_timeout(&self) -> Option<Duration> {
        self.dht_request_timeout
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            testnet: false,
            port: Some(0),
            bootstrap: None,
            domain: None,
            storage: storage(None)
                .expect("operating environment provides no directory for application data"),
            keypair: Keypair::random(),
            dht_request_timeout: None,
            default_list_limit: DEFAULT_LIST_LIMIT,
            max_list_limit: DEFAULT_MAX_LIST_LIMIT,
        }
    }
}

fn deserialize_secret_key(s: String) -> anyhow::Result<[u8; 32]> {
    let bytes =
        hex::decode(s).map_err(|_| anyhow!("secret_key in config.toml should hex encoded"))?;

    if bytes.len() != 32 {
        return Err(anyhow!(format!(
            "secret_key in config.toml should be 32 bytes in hex (64 characters), got: {}",
            bytes.len()
        )));
    }

    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);

    Ok(arr)
}

fn storage(storage: Option<String>) -> Result<PathBuf> {
    let dir = if let Some(storage) = storage {
        PathBuf::from(storage)
    } else {
        let path = dirs_next::data_dir().ok_or_else(|| {
            anyhow!("operating environment provides no directory for application data")
        })?;
        path.join(DEFAULT_STORAGE_DIR)
    };

    Ok(dir.join("homeserver"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        Config::try_from_str("").unwrap();
    }
}
