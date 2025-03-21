//! Configuration for the server
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
};


use super::{default_toml::DEFAULT_CONFIG, validate_domain::validate_domain};

/// All configuration related to the DHT
/// and pkdns.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct PkdnsToml {
    #[serde(default = "default_public_ip")]
    pub public_ip: IpAddr,

    #[serde(default = "default_public_port")]
    pub public_port: Option<u16>,

    /// The list of bootstrap nodes for the DHT. If None, the default pkarr bootstrap nodes will be used.
    #[serde(default = "default_dht_bootstrap_nodes")]
    pub dht_bootstrap_nodes: Option<Vec<String>>,
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


/// All configuration related to the HTTP API
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct HttpApiToml {
    #[serde(default = "default_http_port")]
    pub listen_http_port: u16,
    #[serde(default = "default_https_port")]
    pub listen_https_port: u16,

    #[serde(deserialize_with = "validate_domain", default = "default_legacy_browser_domain")]
    pub legacy_browser_domain: Option<String>,
}

fn default_legacy_browser_domain() -> Option<String> {
    None
}

fn default_http_port() -> u16 {
    6286
}

fn default_https_port() -> u16 {
    6287
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

    #[serde(default = "default_signup_mode", deserialize_with = "validate_signup_mode")]
    signup_mode: String,

    #[serde(default = "default_admin_password")]
    admin_password: String,

    http_api: HttpApiToml,
    pkdns: PkdnsToml,
}

fn default_signup_mode() -> String {
    "token_required".to_string()
}

fn default_admin_password() -> String {
    "admin".to_string()
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

    /// Returns the default config including comments as a string.
    /// toml lib can't handle comments, so we maintain this manually.
    pub fn default_string() -> &'static str {
        DEFAULT_CONFIG
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
    fn parse_config() {
        let config: ConfigToml = ConfigToml::try_from(DEFAULT_CONFIG).expect("Failed to parse config");
    
        // Verify Http api config
        let http_api = config.http_api;
        assert_eq!(http_api.listen_http_port, 6286);
        assert_eq!(http_api.listen_https_port, 6287);
        
        // Verify domain config
        assert_eq!(http_api.legacy_browser_domain, Some("example.com".to_string()));

        // Verify pkdns config
        assert_eq!(config.pkdns.public_ip, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(config.pkdns.public_port, Some(6286));
        assert_eq!(config.pkdns.dht_bootstrap_nodes, Some(vec![
            "router.bittorrent.com:6881",
            "dht.transmissionbt.com:6881",
            "dht.libtorrent.org:25401",
            "relay.pkarr.org:6881"
        ].iter().map(|s| s.to_string()).collect()));
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
