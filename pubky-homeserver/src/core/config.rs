//! Configuration for the server

use anyhow::{anyhow, Context, Result};
use pkarr::Keypair;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_HTTP_PORT: u16 = 6286;
const DEFAULT_HTTPS_PORT: u16 = 6287;

// === Database ===
const DEFAULT_STORAGE_DIR: &str = "pubky";
pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

// === Server ==
pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct DatabaseToml {
    storage: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct ReverseProxyToml {
    pub public_port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct LegacyBrowsersTompl {
    pub domain: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
struct IoToml {
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub public_ip: Option<IpAddr>,

    pub reverse_proxy: Option<ReverseProxyToml>,
    pub legacy_browsers: Option<LegacyBrowsersTompl>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct ConfigToml {
    secret_key: Option<String>,

    database: Option<DatabaseToml>,
    io: Option<IoToml>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoConfig {
    pub http_port: u16,
    pub https_port: u16,
    pub public_addr: Option<SocketAddr>,
    pub domain: Option<String>,
}

/// Server configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Run in [testnet](crate::Homeserver::start_testnet) mode.
    pub testnet: bool,
    /// Bootstrapping DHT nodes.
    ///
    /// Helpful to run the server locally or in testnet.
    pub bootstrap: Option<Vec<String>>,
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

    pub io: IoConfig,
}

impl Config {
    fn try_from_str(value: &str) -> Result<Self> {
        let config_toml: ConfigToml = toml::from_str(value)?;

        config_toml.try_into()
    }

    /// Load the config from a file.
    pub async fn load(path: impl AsRef<Path>) -> Result<Config> {
        let config_file_path = path.as_ref();

        let s = tokio::fs::read_to_string(config_file_path)
            .await
            .with_context(|| format!("failed to read {}", path.as_ref().to_string_lossy()))?;

        let mut config = Config::try_from_str(&s)?;

        // support relative path.
        if config.storage.is_relative() {
            config.storage = config_file_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(config.storage.clone());
        }

        fs::create_dir_all(&config.storage)?;
        config.storage = config.storage.canonicalize()?;

        Ok(config)
    }

    /// Test configurations
    pub fn test(testnet: &mainline::Testnet) -> Self {
        let bootstrap = Some(testnet.bootstrap.to_owned());
        let storage = std::env::temp_dir()
            .join(pubky_common::timestamp::Timestamp::now().to_string())
            .join(DEFAULT_STORAGE_DIR);

        Self {
            bootstrap,
            storage,
            db_map_size: 10485760,
            io: IoConfig {
                http_port: 0,
                https_port: 0,
                public_addr: None,
                domain: None,
            },
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            testnet: false,
            keypair: Keypair::random(),
            bootstrap: None,
            storage: storage(None)
                .expect("operating environment provides no directory for application data"),
            dht_request_timeout: None,
            default_list_limit: DEFAULT_LIST_LIMIT,
            max_list_limit: DEFAULT_MAX_LIST_LIMIT,
            db_map_size: DEFAULT_MAP_SIZE,
            io: IoConfig {
                https_port: DEFAULT_HTTPS_PORT,
                http_port: DEFAULT_HTTP_PORT,
                domain: None,
                public_addr: None,
            },
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
            let dir =
                if let Some(storage) = value.database.as_ref().and_then(|db| db.storage.clone()) {
                    storage
                } else {
                    let path = dirs_next::data_dir().ok_or_else(|| {
                        anyhow!("operating environment provides no directory for application data")
                    })?;
                    path.join(DEFAULT_STORAGE_DIR)
                };

            dir.join("homeserver")
        };

        let io = if let Some(io) = value.io {
            IoConfig {
                http_port: io.http_port.unwrap_or(DEFAULT_HTTP_PORT),
                https_port: io.https_port.unwrap_or(DEFAULT_HTTPS_PORT),
                domain: io.legacy_browsers.and_then(|l| l.domain),
                public_addr: io.public_ip.map(|ip| {
                    SocketAddr::from((
                        ip,
                        io.reverse_proxy
                            .and_then(|r| r.public_port)
                            .unwrap_or(io.https_port.unwrap_or(0)),
                    ))
                }),
            }
        } else {
            IoConfig {
                http_port: DEFAULT_HTTP_PORT,
                https_port: DEFAULT_HTTPS_PORT,
                domain: None,
                public_addr: None,
            }
        };

        Ok(Config {
            testnet: false,
            keypair,

            storage,
            dht_request_timeout: None,
            bootstrap: None,
            default_list_limit: DEFAULT_LIST_LIMIT,
            max_list_limit: DEFAULT_MAX_LIST_LIMIT,
            db_map_size: DEFAULT_MAP_SIZE,

            io,
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
    use mainline::Testnet;

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

    #[tokio::test]
    async fn config_load() {
        let crate_dir = std::env::current_dir().unwrap();
        let config_file_path = crate_dir.join("./src/config.example.toml");
        let canonical_file_path = config_file_path.canonicalize().unwrap();

        let config = Config::load(canonical_file_path).await.unwrap();

        assert!(config
            .storage
            .ends_with("pubky-homeserver/src/storage/homeserver"));
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

                io: IoConfig {
                    http_port: 0,
                    https_port: 0,
                    public_addr: None,
                    domain: None
                },
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

[database]
# Storage directory Defaults to <System's Data Directory>
# storage = ""

[io]
# The port number to run an HTTP (clear text) server on.
http_port = 6286
# The port number to run an HTTPs (Pkarr TLS) server on.
https_port = 6287

# The public IP of this server.
# 
# This address will be mentioned in the Pkarr records of this
#   Homeserver that is published on its public key (derivde from `secret_key`)
public_ip = "127.0.0.1"

# If you are running this server behind a reverse proxy,
#   you need to provide some extra configurations.
[io.reverse_proxy]
# The public port should be mapped to the `io::https_port`
#   and you should setup tcp forwarding (don't terminate TLS on that port).
public_port = 6287

# If you want your server to be accessible from legacy browsers,
#   you need to provide some extra configurations.
[io.legacy_browsers]
# An ICANN domain name is necessary to support legacy browsers
#
# Make sure to setup a domain name and point it the IP
#   address of this machine where you are running this server.
#
# This domain should point to the `<public_ip>:<http_port>`.
# 
# Currently we don't support ICANN TLS, so you should be runing
#   a reverse proxy and managing certifcates there for this endpoint.
domain = "example.com"
        "#,
        )
        .unwrap();

        assert_eq!(config.keypair, Keypair::from_secret_key(&[0; 32]));
        assert_eq!(config.io.https_port, 6287);
        assert_eq!(
            config.io.public_addr,
            Some(SocketAddr::from(([127, 0, 0, 1], 6287)))
        );
        assert_eq!(config.io.domain, Some("example.com".to_string()));
    }
}
