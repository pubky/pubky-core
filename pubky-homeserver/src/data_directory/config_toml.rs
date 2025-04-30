//! Configuration file for the homeserver.
//!
//! All default values live exclusively in `config.default.toml`.
//! This module embeds that file at compile-time, parses it once,
//! and lets callers optionally layer their own TOML on top.

use super::{domain_port::DomainPort, Domain, SignupMode};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    fs,
    net::{IpAddr, SocketAddr},
    num::NonZeroU64,
    path::Path,
    str::FromStr,
};
use url::Url;

/// Embedded copy of the default configuration (single source of truth for defaults)
pub const DEFAULT_CONFIG: &str = include_str!("../../config.default.toml");

/// Parsed, ready-to-clone version of the default config.
/// Parsing happens exactly once at program start-up.
static BASE_CONFIG: Lazy<ConfigToml> = Lazy::new(|| {
    ConfigToml::from_str(DEFAULT_CONFIG).expect("Embedded config.default.toml must be valid")
});

/// Helper: merge two arbitrary `toml::Value` trees (recursive)
fn merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    use toml::Value::{Array, Table};

    match (base, overlay) {
        (Table(b), Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(bv) => merge_toml(bv, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        // For arrays we simply replace; customise if “extend” semantics are preferred.
        (Array(b), Array(o)) => {
            *b = o;
        }
        // Scalar or type mismatch → overlay wins.
        (b, o) => *b = o,
    }
}

/// Error that can occur when reading a configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigReadError {
    /// The file did not exist or could not be read.
    #[error("config file not found: {0}")]
    ConfigFileNotFound(#[from] std::io::Error),
    /// The TOML was syntactically invalid.
    #[error("config file is not valid TOML: {0}")]
    ConfigFileNotValid(#[from] toml::de::Error),
}

/// Config structs

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PkdnsToml {
    pub public_ip: IpAddr,
    pub public_pubky_tls_port: Option<u16>,
    pub public_icann_http_port: Option<u16>,
    pub icann_domain: Option<Domain>,
    pub user_keys_republisher_interval: u64,
    pub dht_bootstrap_nodes: Option<Vec<DomainPort>>,
    pub dht_relay_nodes: Option<Vec<Url>>,
    pub dht_request_timeout_ms: Option<NonZeroU64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DriveToml {
    pub pubky_listen_socket: SocketAddr,
    pub icann_listen_socket: SocketAddr,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminToml {
    pub listen_socket: SocketAddr,
    pub admin_password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct GeneralToml {
    pub signup_mode: SignupMode,
    pub lmdb_backup_interval_s: u64,
    pub user_storage_quota_mb: u64,
}

/// The overall application configuration, composed of several subsections.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ConfigToml {
    /// General application settings (signup mode, quotas, backups).
    pub general: GeneralToml,
    /// File‐drive API settings (listen sockets for Pubky TLS and HTTP).
    pub drive: DriveToml,
    /// Administrative API settings (listen socket and password).
    pub admin: AdminToml,
    /// Peer‐to‐peer DHT / PKDNS settings (public endpoints, bootstrap, relays).
    pub pkdns: PkdnsToml,
}

impl Default for ConfigToml {
    fn default() -> Self {
        BASE_CONFIG.to_owned()
    }
}

impl Default for DriveToml {
    fn default() -> Self {
        ConfigToml::default().drive
    }
}

impl Default for AdminToml {
    fn default() -> Self {
        ConfigToml::default().admin
    }
}

impl Default for PkdnsToml {
    fn default() -> Self {
        ConfigToml::default().pkdns
    }
}

impl ConfigToml {
    /// Read and parse a configuration file, overlaying it on top of the embedded defaults.
    ///
    /// # Arguments
    /// * `path` - The path to the TOML configuration file
    ///
    /// # Returns
    /// * `Result<ConfigToml>` - The parsed configuration or an error if reading/parsing fails
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigReadError> {
        let raw = fs::read_to_string(path)?;
        Self::from_raw_str(&raw)
    }

    /// Parse a raw TOML string, overlaying it on top of the embedded defaults.
    pub fn from_raw_str(raw: &str) -> Result<Self, ConfigReadError> {
        // 1. Start with the default tree.
        let mut merged: toml::Value = DEFAULT_CONFIG
            .parse()
            .expect("embedded defaults invalid TOML");

        // 2. Parse and merge the user's overrides.
        let user_val: toml::Value = raw.parse()?;
        merge_toml(&mut merged, user_val);

        // 3. Deserialize into our strong types.
        merged
            .try_into()
            .map_err(ConfigReadError::ConfigFileNotValid)
    }

    /// Render the embedded default config but comment out every value,
    /// producing a handy template for end-users.
    pub fn default_string() -> String {
        DEFAULT_CONFIG
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let is_title = trimmed.starts_with('[');
                let is_comment = trimmed.starts_with('#');
                if !is_title && !is_comment && !trimmed.is_empty() {
                    format!("# {}", line)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Returns a default config tuned for unit tests.
    pub fn test() -> Self {
        let mut config = Self::default();
        config.general.signup_mode = SignupMode::Open;
        // Use ephemeral ports (0) so parallel tests don’t collide.
        config.drive.icann_listen_socket = SocketAddr::from(([127, 0, 0, 1], 0));
        config.drive.pubky_listen_socket = SocketAddr::from(([127, 0, 0, 1], 0));
        config.admin.listen_socket = SocketAddr::from(([127, 0, 0, 1], 0));
        config.pkdns.icann_domain =
            Some(Domain::from_str("localhost").expect("localhost is a valid domain"));
        config.pkdns.dht_relay_nodes = None;
        config.pkdns.dht_request_timeout_ms = None;
        config.pkdns.dht_bootstrap_nodes = None;
        config
    }
}

impl FromStr for ConfigToml {
    type Err = toml::de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        toml::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
        num::NonZeroU64,
        str::FromStr,
    };

    #[test]
    fn test_default_config() {
        let c = ConfigToml::default();
        assert_eq!(c.general.signup_mode, SignupMode::TokenRequired);
        assert_eq!(c.general.user_storage_quota_mb, 0);
        assert_eq!(c.general.lmdb_backup_interval_s, 0);
        assert_eq!(
            c.drive.icann_listen_socket,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6286))
        );
        assert_eq!(
            c.pkdns.icann_domain,
            Some(Domain::from_str("localhost").unwrap())
        );
        assert_eq!(
            c.drive.pubky_listen_socket,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6287))
        );
        assert_eq!(
            c.admin.listen_socket,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 6288))
        );
        assert_eq!(c.admin.admin_password, "admin");
        assert_eq!(c.pkdns.public_ip, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(c.pkdns.public_pubky_tls_port, None);
        assert_eq!(c.pkdns.public_icann_http_port, None);
        assert_eq!(c.pkdns.user_keys_republisher_interval, 14400);
        let dht_bootstrap_nodes = vec![
            DomainPort::from_str("router.bittorrent.com:6881").unwrap(),
            DomainPort::from_str("dht.transmissionbt.com:6881").unwrap(),
            DomainPort::from_str("dht.libtorrent.org:25401").unwrap(),
            DomainPort::from_str("relay.pkarr.org:6881").unwrap(),
        ];
        assert_eq!(c.pkdns.dht_bootstrap_nodes, Some(dht_bootstrap_nodes));
        let dht_relay_nodes = vec![
            Url::parse("https://relay.pkarr.org").unwrap(),
            Url::parse("https://pkarr.pubky.org").unwrap(),
        ];
        assert_eq!(c.pkdns.dht_relay_nodes, Some(dht_relay_nodes));
        assert_eq!(c.pkdns.dht_request_timeout_ms, NonZeroU64::new(2000));
    }

    #[test]
    fn test_default_config_commented_out() {
        // Sanity check that the default config is valid even when the variables are commented out.
        // An empty or fully commented out .toml should still be equal to the default ConfigToml
        let s = ConfigToml::default_string();
        let parsed: ConfigToml = ConfigToml::from_raw_str(&s).expect("Should be parseable");
        assert_eq!(parsed, ConfigToml::default());
    }

    #[test]
    fn test_empty_config() {
        // Test that a minimal config with only the general section works
        let s = "[general]\nsignup_mode = \"open\"\n";
        let parsed: ConfigToml = ConfigToml::from_raw_str(s).unwrap();
        // Check that explicitly set values are preserved
        assert_eq!(parsed.general.signup_mode, SignupMode::Open);
        // Other fields that were not set (left empty) should still match the default.
        assert_eq!(parsed.admin, ConfigToml::default().admin);
    }
}
