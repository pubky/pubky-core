//!
//! Configuration file for the homeserver.
//!
use super::{domain_port::DomainPort, SignupMode};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::NonZeroU64,
    str::FromStr,
};
use url::Url;

/// Default TOML configuration for the homeserver.
/// This is used to create a default config file if it doesn't exist.
/// Why not use the Default trait? The `toml` crate doesn't support adding comments.
/// So we maintain this default manually.
pub const DEFAULT_CONFIG: &str = include_str!("../../config.default.toml");

/// All configuration related to the DHT
/// and /pkarr.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PkdnsToml {
    /// The public IP address and port of the server to be advertised in the DHT.
    #[serde(default = "default_public_socket")]
    pub public_socket: SocketAddr,

    /// The interval at which the user keys are republished in the DHT.
    #[serde(default = "default_user_keys_republisher_interval")]
    pub user_keys_republisher_interval: NonZeroU64,

    /// The list of bootstrap nodes for the DHT. If None, the default pkarr bootstrap nodes will be used.
    #[serde(default)]
    pub dht_bootstrap_nodes: Option<Vec<DomainPort>>,

    /// The list of relay nodes for the DHT. If None, the default pkarr relay nodes will be used.
    #[serde(default)]
    pub dht_relay_nodes: Option<Vec<Url>>,
}

fn default_public_socket() -> SocketAddr {
    let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let port = 6286;
    SocketAddr::from((ip, port))
}

fn default_user_keys_republisher_interval() -> NonZeroU64 {
    // 4 hours
    NonZeroU64::new(14400).expect("14400 is a valid non-zero u64")
}

fn default_pubky_drive_listen_socket() -> SocketAddr {
    let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let port = 6287;
    SocketAddr::from((ip, port))
}

/// All configuration related to file drive
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DriveToml {
    /// The port on which the Pubky TLS Drive API will listen.
    #[serde(default = "default_pubky_drive_listen_socket")]
    pub pubky_listen_socket: SocketAddr,
    /// The port on which the regular http API will listen.
    #[serde(default = "default_icann_drive_listen_socket")]
    pub icann_listen_socket: SocketAddr,
    /// Optional domain name of the regular http API.
    #[serde(default)]
    pub icann_domain: Option<String>,
}

fn default_icann_drive_listen_socket() -> SocketAddr {
    let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let port = 6286;
    SocketAddr::from((ip, port))
}

/// All configuration related to the admin API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminToml {
    /// The socket on which the admin API will listen.
    #[serde(default = "default_admin_listen_socket")]
    pub listen_socket: SocketAddr,
    /// The password for the admin API.
    #[serde(default = "default_admin_password")]
    pub admin_password: String,
}

fn default_admin_password() -> String {
    "admin".to_string()
}

fn default_admin_listen_socket() -> SocketAddr {
    let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let port = 6288;
    SocketAddr::from((ip, port))
}

/// All configuration related to the admin API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct GeneralToml {
    /// The mode of the signup.
    #[serde(default)]
    pub signup_mode: SignupMode,
    /// LMDB backup interval in seconds. 0 means disabled.
    #[serde(default)]
    pub lmdb_backup_interval_s: u64,
}

/// The error that can occur when reading the config file
#[derive(Debug, thiserror::Error)]
pub enum ConfigReadError {
    /// The config file not found
    #[error("Config file not found. {0}")]
    ConfigFileNotFound(#[from] std::io::Error),
    /// The config file is not valid    
    #[error("Config file is not valid. {0}")]
    ConfigFileNotValid(#[from] toml::de::Error),
}

/// The main server configuration
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ConfigToml {
    /// The configuration for the general settings.
    pub general: GeneralToml,
    /// The configuration for the drive files.
    pub drive: DriveToml,
    /// The configuration for the admin API.
    pub admin: AdminToml,
    /// The configuration for the pkdns.
    pub pkdns: PkdnsToml,
}

impl ConfigToml {
    /// Reads the configuration from a TOML file at the specified path.
    ///
    /// # Arguments
    /// * `path` - The path to the TOML configuration file
    ///
    /// # Returns
    /// * `Result<ConfigToml>` - The parsed configuration or an error if reading/parsing fails
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, ConfigReadError> {
        let contents = std::fs::read_to_string(path)?;
        let config: ConfigToml = contents.parse()?;
        Ok(config)
    }

    /// Returns the default config with all variables commented out.
    pub fn default_string() -> String {
        // Comment out all variables so they are not fixed by default.
        DEFAULT_CONFIG
            .split("\n")
            .map(|line| {
                let is_not_commented_variable =
                    !line.starts_with("#") && !line.starts_with("[") && line.is_empty();
                if is_not_commented_variable {
                    format!("# {}", line)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("\n")
    }
}

impl Default for ConfigToml {
    fn default() -> Self {
        DEFAULT_CONFIG
            .parse()
            .expect("Default config is always valid")
    }
}

impl FromStr for ConfigToml {
    type Err = toml::de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: ConfigToml = toml::from_str(s)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let c: ConfigToml = ConfigToml::default();

        assert_eq!(c.general.signup_mode, SignupMode::TokenRequired);
        assert_eq!(
            c.drive.icann_listen_socket,
            default_icann_drive_listen_socket()
        );
        assert_eq!(c.drive.icann_domain, Some("example.com".to_string()));

        assert_eq!(
            c.drive.pubky_listen_socket,
            default_pubky_drive_listen_socket()
        );

        assert_eq!(c.admin.listen_socket, default_admin_listen_socket());
        assert_eq!(c.admin.admin_password, default_admin_password());

        // Verify pkdns config
        assert_eq!(c.pkdns.public_socket, default_public_socket());
        assert_eq!(
            c.pkdns.user_keys_republisher_interval,
            default_user_keys_republisher_interval()
        );
        assert_eq!(
            c.pkdns.dht_bootstrap_nodes,
            Some(vec![
                DomainPort::from_str("router.bittorrent.com:6881").unwrap(),
                DomainPort::from_str("dht.transmissionbt.com:6881").unwrap(),
                DomainPort::from_str("dht.libtorrent.org:25401").unwrap(),
                DomainPort::from_str("relay.pkarr.org:6881").unwrap(),
            ])
        );
        assert_eq!(
            c.pkdns.dht_relay_nodes,
            Some(vec![
                Url::parse("https://relay.pkarr.org").unwrap(),
                Url::parse("https://pkarr.pubky.org").unwrap(),
            ])
        );
    }

    #[test]
    fn test_default_config_commented_out() {
        // Sanity check that the default config is valid
        // even when the variables are commented out.
        let s = ConfigToml::default_string();
        let _: ConfigToml = s.parse().expect("Failed to parse config");
    }
}
