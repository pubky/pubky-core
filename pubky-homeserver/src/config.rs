//! Configuration for the server

use anyhow::{anyhow, Context, Result};
use pkarr::Keypair;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf}, time::Duration,
};

use crate::{core::CoreConfig, io::IoConfig};


pub const DEFAULT_REPUBLISHER_INTERVAL: u64 = 4 * 60 * 60; // 4 hours in seconds

// === Core ==
pub const DEFAULT_STORAGE_DIR: &str = "pubky";
pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

// === IO ===
pub const DEFAULT_HTTP_PORT: u16 = 6286;
pub const DEFAULT_HTTPS_PORT: u16 = 6287;

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
    #[serde(default = "default_republisher_interval")]
    user_keys_republisher_interval: u64,

    database: Option<DatabaseToml>,
    io: Option<IoToml>,
}

fn default_republisher_interval() -> u64 {
    DEFAULT_REPUBLISHER_INTERVAL
}

/// Server configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Server keypair.
    ///
    /// Defaults to a random keypair.
    pub keypair: Keypair,

    pub io: IoConfig,
    pub core: CoreConfig,
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
        if config.core.storage.is_relative() {
            config.core.storage = config_file_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(config.core.storage.clone());
        }

        fs::create_dir_all(&config.core.storage)?;
        config.core.storage = config.core.storage.canonicalize()?;

        Ok(config)
    }

    /// Create test configurations
    pub fn test(bootstrap: &[String]) -> Self {
        let bootstrap = Some(bootstrap.to_vec());

        Self {
            io: IoConfig {
                bootstrap,
                http_port: 0,
                https_port: 0,

                ..Default::default()
            },
            core: CoreConfig::test(),
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keypair: Keypair::random(),
            io: IoConfig::default(),
            core: CoreConfig::default(),
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
                ..Default::default()
            }
        } else {
            IoConfig {
                http_port: DEFAULT_HTTP_PORT,
                https_port: DEFAULT_HTTPS_PORT,
                ..Default::default()
            }
        };

        let user_keys_republisher_interval = if value.user_keys_republisher_interval > 0 {
            Some(Duration::from_secs(value.user_keys_republisher_interval))
        } else {
            None
        };

        Ok(Config {
            keypair,
            io,
            core: CoreConfig {
                storage,
                user_keys_republisher_interval,
                ..Default::default()
            },
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

#[cfg(test)]
mod tests {

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
            .core
            .storage
            .ends_with("pubky-homeserver/src/storage/homeserver"));
    }

    #[test]
    fn config_test() {
        let config = Config::test(&[]);

        assert_eq!(
            config,
            Config {
                keypair: config.keypair.clone(),

                io: IoConfig {
                    bootstrap: Some(vec![]),
                    http_port: 0,
                    https_port: 0,

                    ..Default::default()
                },
                core: CoreConfig {
                    db_map_size: 10485760,
                    storage: config.core.storage.clone(),

                    ..Default::default()
                },
            }
        )
    }

    #[test]
    fn parse() {
        let config = Config::try_from_str(
            r#"
# Secret key (in hex) to generate the Homeserver's Keypair
secret_key = "0000000000000000000000000000000000000000000000000000000000000000"

# The interval at which user keys are republished to the DHT.
user_keys_republisher_interval = 3600  # 1 hour in seconds

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
# Currently we don't support ICANN TLS, so you should be running
#   a reverse proxy and managing certificates there for this endpoint.
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
        assert_eq!(config.core.user_keys_republisher_interval, Some(Duration::from_secs(3600))); // 1 hour
    }
}
