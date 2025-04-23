//!
//! Configuration file for the homeserver.
//!
use super::{domain_port::DomainPort, Domain, SignupMode};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
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
    #[serde(default = "default_public_ip")]
    pub public_ip: IpAddr,

    /// The public port of the Pubky TLS Drive API in case it's different from the listening port.
    #[serde(default)]
    pub public_pubky_tls_port: Option<u16>,

    /// The public port of the regular http API in case it's different from the listening port.
    #[serde(default)]
    pub public_icann_http_port: Option<u16>,

    /// Optional domain name of the regular http API.
    #[serde(default)]
    pub icann_domain: Option<Domain>,

    /// The interval at which the user keys are republished in the DHT.
    /// 0 means disabled.
    #[serde(default = "default_user_keys_republisher_interval")]
    pub user_keys_republisher_interval: u64,

    /// The list of bootstrap nodes for the DHT. If None, the default pkarr bootstrap nodes will be used.
    #[serde(default = "default_dht_bootstrap_nodes")]
    pub dht_bootstrap_nodes: Option<Vec<DomainPort>>,

    /// The list of relay nodes for the DHT.
    /// If not set and no bootstrap nodes are set, the default pkarr relay nodes will be used.
    #[serde(default = "default_dht_relay_nodes")]
    pub dht_relay_nodes: Option<Vec<Url>>,

    /// The request timeout for the DHT. If None, the default pkarr request timeout will be used.
    #[serde(default = "default_dht_request_timeout")]
    pub dht_request_timeout_ms: Option<NonZeroU64>,
}

impl Default for PkdnsToml {
    fn default() -> Self {
        Self {
            public_ip: default_public_ip(),
            public_pubky_tls_port: Option::default(),
            public_icann_http_port: Option::default(),
            icann_domain: Option::default(),
            user_keys_republisher_interval: default_user_keys_republisher_interval(),
            dht_bootstrap_nodes: default_dht_bootstrap_nodes(),
            dht_relay_nodes: default_dht_relay_nodes(),
            dht_request_timeout_ms: default_dht_request_timeout(),
        }
    }
}

fn default_public_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

fn default_dht_bootstrap_nodes() -> Option<Vec<DomainPort>> {
    None
}

fn default_dht_relay_nodes() -> Option<Vec<Url>> {
    None
}

fn default_dht_request_timeout() -> Option<NonZeroU64> {
    None
}

fn default_user_keys_republisher_interval() -> u64 {
    // 4 hours
    14400
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
}

impl Default for DriveToml {
    fn default() -> Self {
        Self {
            pubky_listen_socket: default_pubky_drive_listen_socket(),
            icann_listen_socket: default_icann_drive_listen_socket(),
        }
    }
}

fn default_pubky_drive_listen_socket() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6287))
}

fn default_icann_drive_listen_socket() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6286))
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

impl Default for AdminToml {
    fn default() -> Self {
        Self {
            listen_socket: default_admin_listen_socket(),
            admin_password: default_admin_password(),
        }
    }
}

fn default_admin_password() -> String {
    "admin".to_string()
}

fn default_admin_listen_socket() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6288))
}

/// All configuration related to the admin API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
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
    #[serde(default)]
    pub general: GeneralToml,
    /// The configuration for the drive files.
    #[serde(default)]
    pub drive: DriveToml,
    /// The configuration for the admin API.
    #[serde(default)]
    pub admin: AdminToml,
    /// The configuration for the pkdns.
    #[serde(default)]
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
                let is_title = line.starts_with("[");
                let is_comment = line.starts_with("#");
                let is_empty = line.is_empty();

                let is_other = !is_title && !is_comment && !is_empty;
                if is_other {
                    format!("# {}", line)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Returns a default config appropriate for testing.
    pub fn test() -> Self {
        let mut config = Self::default();
        // For easy testing, we set the signup mode to open.
        config.general.signup_mode = SignupMode::Open;
        // Set the listen ports to randomly available ports so they don't conflict.
        config.drive.icann_listen_socket = SocketAddr::from(([127, 0, 0, 1], 0));
        config.drive.pubky_listen_socket = SocketAddr::from(([127, 0, 0, 1], 0));
        config.admin.listen_socket = SocketAddr::from(([127, 0, 0, 1], 0));
        config.pkdns.icann_domain =
            Some(Domain::from_str("localhost").expect("localhost is a valid domain"));
        config
    }
}

impl Default for ConfigToml {
    fn default() -> Self {
        ConfigToml::default_string()
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
        assert_eq!(c.general.lmdb_backup_interval_s, 0);
        assert_eq!(
            c.drive.icann_listen_socket,
            default_icann_drive_listen_socket()
        );
        assert_eq!(c.pkdns.icann_domain, None);

        assert_eq!(
            c.drive.pubky_listen_socket,
            default_pubky_drive_listen_socket()
        );

        assert_eq!(c.admin.listen_socket, default_admin_listen_socket());
        assert_eq!(c.admin.admin_password, default_admin_password());

        // Verify pkdns config
        assert_eq!(c.pkdns.public_ip, default_public_ip());
        assert_eq!(c.pkdns.public_pubky_tls_port, None);
        assert_eq!(c.pkdns.public_icann_http_port, None);
        assert_eq!(
            c.pkdns.user_keys_republisher_interval,
            default_user_keys_republisher_interval()
        );
        assert_eq!(c.pkdns.dht_bootstrap_nodes, None);
        assert_eq!(c.pkdns.dht_relay_nodes, None);

        assert_eq!(c.pkdns.dht_request_timeout_ms, None);
    }

    #[test]
    fn test_default_config_commented_out() {
        // Sanity check that the default config is valid
        // even when the variables are commented out.
        let s = ConfigToml::default_string();
        let parsed: ConfigToml = s.parse().expect("Failed to parse config");
        assert_eq!(
            parsed.pkdns.dht_bootstrap_nodes, None,
            "dht_bootstrap_nodes not commented out"
        );
    }

    #[test]
    fn test_empty_config() {
        // Test that a minimal config with only the general section works
        let s = "[general]
        signup_mode = \"open\"
        ";
        let parsed: ConfigToml = s.parse().unwrap();

        // Check that explicitly set values are preserved
        assert_eq!(
            parsed.general.signup_mode,
            SignupMode::Open,
            "signup_mode not set correctly"
        );
    }
}
