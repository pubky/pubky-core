//! Configuration for the server

use anyhow::{anyhow, Context, Result};
use pkarr::Keypair;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    time::Duration,
};

// === Database ===
const DEFAULT_STORAGE_DIR: &str = "pubky";
pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

// === Server ==
pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct ConfigToml {
    port: Option<u16>,
    bootstrap: Option<Vec<String>>,
    domain: Option<String>,
    storage: Option<PathBuf>,
    secret_key: Option<String>,
    dht_request_timeout: Option<Duration>,
    default_list_limit: Option<u16>,
    max_list_limit: Option<u16>,
    db_map_size: Option<usize>,
}

/// Server configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The configured port for this server.
    pub port: u16,
    /// Bootstrapping DHT nodes.
    ///
    /// Helpful to run the server locally or in testnet.
    pub bootstrap: Option<Vec<String>>,
    /// A public domain for this server
    /// necessary for web browsers running in https environment.
    pub domain: Option<String>,
    /// Path to the storage directory.
    ///
    /// Defaults to a directory in the OS data directory
    pub storage: PathBuf,
    /// Server keypair.
    ///
    /// Defaults to a random keypair.
    pub keypair: Keypair,
    pub dht_request_timeout: Option<Duration>,
    /// The default limit of a list api if no `limit` query parameter is provided.
    ///
    /// Defaults to `100`
    pub default_list_limit: u16,
    /// The maximum limit of a list api, even if a `limit` query parameter is provided.
    ///
    /// Defaults to `1000`
    pub max_list_limit: u16,

    // === Database params ===
    pub db_map_size: usize,
}

impl Config {
    fn try_from_str(value: &str) -> Result<Self> {
        let config_toml: ConfigToml = toml::from_str(value)?;

        config_toml.try_into()
    }

    /// Load the config from a file.
    pub async fn load(path: impl AsRef<Path>) -> Result<Config> {
        let s = tokio::fs::read_to_string(path.as_ref())
            .await
            .with_context(|| format!("failed to read {}", path.as_ref().to_string_lossy()))?;

        Config::try_from_str(&s)
    }

    /// Test configurations
    pub fn test(testnet: &pkarr::mainline::Testnet) -> Self {
        let bootstrap = Some(testnet.bootstrap.to_owned());
        let storage = std::env::temp_dir()
            .join(pubky_common::timestamp::Timestamp::now().to_string())
            .join(DEFAULT_STORAGE_DIR);

        Self {
            bootstrap,
            storage,
            db_map_size: 10485760,
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 0,
            bootstrap: None,
            domain: None,
            storage: storage(None)
                .expect("operating environment provides no directory for application data"),
            keypair: Keypair::random(),
            dht_request_timeout: None,
            default_list_limit: DEFAULT_LIST_LIMIT,
            max_list_limit: DEFAULT_MAX_LIST_LIMIT,
            db_map_size: DEFAULT_MAP_SIZE,
        }
    }
}

impl TryFrom<ConfigToml> for Config {
    type Error = anyhow::Error;

    fn try_from(value: ConfigToml) -> std::result::Result<Self, Self::Error> {
        let keypair = if let Some(secret_key) = value.secret_key {
            let secret_key = deserialize_secret_key(secret_key)?;
            Keypair::from_secret_key(&secret_key)
        } else {
            Keypair::random()
        };

        let storage = {
            let dir = if let Some(storage) = value.storage {
                storage
            } else {
                let path = dirs_next::data_dir().ok_or_else(|| {
                    anyhow!("operating environment provides no directory for application data")
                })?;
                path.join(DEFAULT_STORAGE_DIR)
            };

            dir.join("homeserver")
        };

        Ok(Config {
            port: value.port.unwrap_or(0),
            bootstrap: value.bootstrap,
            domain: value.domain,
            keypair,
            storage,
            dht_request_timeout: value.dht_request_timeout,
            default_list_limit: value.default_list_limit.unwrap_or(DEFAULT_LIST_LIMIT),
            max_list_limit: value.default_list_limit.unwrap_or(DEFAULT_MAX_LIST_LIMIT),
            db_map_size: value.db_map_size.unwrap_or(DEFAULT_MAP_SIZE),
        })
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
    use pkarr::mainline::Testnet;

    use super::*;

    #[test]
    fn parse_empty() {
        let config = Config::try_from_str("").unwrap();

        assert_eq!(
            config,
            Config {
                keypair: config.keypair.clone(),
                ..Default::default()
            }
        )
    }

    #[test]
    fn config_test() {
        let testnet = Testnet::new(3).unwrap();
        let config = Config::test(&testnet);

        assert_eq!(
            config,
            Config {
                bootstrap: testnet.bootstrap.into(),
                db_map_size: 10485760,

                storage: config.storage.clone(),
                keypair: config.keypair.clone(),
                ..Default::default()
            }
        )
    }

    #[test]
    fn parse() {
        let config = Config::try_from_str(
            r#"
            # Secret key (in hex) to generate the Homeserver's Keypair
            secret_key = "0000000000000000000000000000000000000000000000000000000000000000"
            # Domain to be published in Pkarr records for this server to be accessible by.
            domain = "localhost"
            # Port for the Homeserver to listen on.
            port = 6287
            # Storage directory Defaults to <System's Data Directory>
            storage = "/homeserver"

            bootstrap = ["foo", "bar"]

            # event stream
            default_list_limit = 500
            max_list_limit = 10000
        "#,
        )
        .unwrap();

        assert_eq!(config.keypair, Keypair::from_secret_key(&[0; 32]));
        assert_eq!(config.port, 6287);
        assert_eq!(
            config.bootstrap,
            Some(vec!["foo".to_string(), "bar".to_string()])
        );
    }
}
