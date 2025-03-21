//! Configuration for the server
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug, net::{IpAddr, Ipv4Addr}, num::NonZeroU64, str::FromStr
};

use super::{default_toml::DEFAULT_CONFIG, validate_domain::validate_domain};

/// All configuration related to the DHT
/// and /pkarr.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PkdnsToml {
    /// The public IP address of the server to be advertised in the DHT.
    #[serde(default = "default_public_ip")]
    pub public_ip: IpAddr,

    /// The public port of the server to be advertised in the DHT.
    #[serde(default = "default_public_port")]
    pub public_port: Option<u16>,

    /// The interval at which the user keys are republished in the DHT.
    #[serde(default = "default_user_keys_republisher_interval")]
    pub user_keys_republisher_interval: NonZeroU64,

    /// The list of bootstrap nodes for the DHT. If None, the default pkarr bootstrap nodes will be used.
    #[serde(default = "default_dht_bootstrap_nodes")]
    pub dht_bootstrap_nodes: Option<Vec<String>>,

    /// The list of relay nodes for the DHT. If None, the default pkarr relay nodes will be used.
    #[serde(default = "default_dht_relay_nodes")]
    pub dht_relay_nodes: Option<Vec<String>>,
}


fn default_public_port() -> Option<u16> {
    None
}

fn default_public_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

fn default_dht_bootstrap_nodes() -> Option<Vec<String>> {
    None
}

fn default_dht_relay_nodes() -> Option<Vec<String>> {
    None
}

fn default_user_keys_republisher_interval() -> NonZeroU64 {
    // 4 hours
    NonZeroU64::new(14400).expect("14400 is a valid non-zero u64")
}

/// All configuration related to the Pubky TLS Drive API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PubkyDriveApiToml {
    /// The port on which the Pubky TLS Drive API will listen.
    #[serde(default = "default_pubky_drive_listen_port")]
    pub listen_port: u16,
}

fn default_pubky_drive_listen_port() -> u16 {
    6287
}

/// All configuration related to the regular HTTP API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IcannDriveApiToml {
    /// The port on which the regular http API will listen.
    #[serde(default = "default_icann_drive_listen_port")]
    pub listen_port: u16,
    /// Optional domain name of the regular http API.
    #[serde(deserialize_with = "validate_domain", default = "default_icann_drive_domain")]
    pub domain: Option<String>,
}

fn default_icann_drive_domain() -> Option<String> {
    None
}

fn default_icann_drive_listen_port() -> u16 {
    6286
}

/// All configuration related to the admin API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminApiToml {
    /// The port on which the admin API will listen.
    #[serde(default = "default_admin_listen_port")]
    pub listen_port: u16,
    /// The password for the admin API.
    #[serde(default = "default_admin_password")]
    pub admin_password: String,
}

fn default_admin_password() -> String {
    "admin".to_string()
}

fn default_admin_listen_port() -> u16 {
    6288
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
    /// The mode of the signup.
    #[serde(default = "default_signup_mode", deserialize_with = "validate_signup_mode")]
    pub signup_mode: String,

    /// The configuration for the regular http API.
    pub icann_drive_api: IcannDriveApiToml,
    /// The configuration for the Pubky TLS Drive API.
    pub pubky_drive_api: PubkyDriveApiToml,
    /// The configuration for the admin API.
    pub admin_api: AdminApiToml,
    /// The configuration for the pkdns.
    pub pkdns: PkdnsToml,
}

fn default_signup_mode() -> String {
    "token_required".to_string()
}

fn validate_signup_mode<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    match s.as_str() {
        "open" | "token_required" => Ok(s),
        _ => Err(serde::de::Error::custom(
            "signup_mode must be either \"open\" or \"token_required\"",
        )),
    }
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
        let config: ConfigToml = ConfigToml::try_from(&contents)?;
        Ok(config)
    }

    /// Returns the default config with all variables commented out.
    pub fn default_string() -> String {
        // Comment out all variables so they are not fixed by default.
        DEFAULT_CONFIG.split("\n").map(|line| {
            let is_not_commented_variable = !line.starts_with("#") && !line.starts_with("[") && line.len() > 0;
            if is_not_commented_variable {
                format!("# {}", line)
            } else {
                line.to_string()
            }
        }).collect::<Vec<String>>().join("\n")
    }
}

impl FromStr for ConfigToml {
    type Err = toml::de::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let config: ConfigToml = toml::from_str(s)?;
        Ok(config)
    }
}

impl TryFrom<&str> for ConfigToml {
    type Error = toml::de::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let config: ConfigToml = toml::from_str(value)?;
        Ok(config)
    }
}

impl TryFrom<&String> for ConfigToml {
    type Error = toml::de::Error;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        let config: ConfigToml = toml::from_str(value)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use crate::config2::default_toml::DEFAULT_CONFIG;

    use super::*;


    #[test]
    fn test_default_config() {
        let c: ConfigToml = ConfigToml::try_from(DEFAULT_CONFIG).expect("Failed to parse config");
    
        assert_eq!(c.icann_drive_api.listen_port, 6286);
        assert_eq!(c.icann_drive_api.domain, Some("example.com".to_string()));
        
        assert_eq!(c.pubky_drive_api.listen_port, 6287);

        assert_eq!(c.admin_api.listen_port, 6288);
        assert_eq!(c.admin_api.admin_password, "admin".to_string());

        // Verify pkdns config
        assert_eq!(c.pkdns.public_ip, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(c.pkdns.public_port, Some(6286));
        assert_eq!(c.pkdns.user_keys_republisher_interval, NonZeroU64::new(14400).unwrap());
        assert_eq!(c.pkdns.dht_bootstrap_nodes, Some(vec![
            "router.bittorrent.com:6881",
            "dht.transmissionbt.com:6881",
            "dht.libtorrent.org:25401",
            "relay.pkarr.org:6881"
        ].iter().map(|s| s.to_string()).collect()));
    }

    #[test]
    fn test_default_config_commented_out() {
        let s = ConfigToml::default_string();
        let _ = ConfigToml::try_from(&s).expect("Failed to parse config");
    }

    #[test]
    fn test_signup_mode_validation() {
        // Test valid values
        let valid_open = r#"
            signup_mode = "open"
            [http_api]
            [pkdns]
        "#;
        assert!(toml::from_str::<ConfigToml>(valid_open).is_ok());

        let valid_token = r#"
            signup_mode = "token_required"
            [http_api]
            [pkdns]
        "#;
        assert!(toml::from_str::<ConfigToml>(valid_token).is_ok());

        // Test invalid value
        let invalid = r#"
            signup_mode = "invalid"
            [http_api]
            [pkdns]
        "#;
        let result = toml::from_str::<ConfigToml>(invalid);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be either"));
    }
}
