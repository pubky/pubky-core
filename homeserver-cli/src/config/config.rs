use serde::{Deserialize, Serialize};
use std::path::Path;
use url::Url;
use anyhow::{Context, Result};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AdminToml {
    /// Password for admin authentication
    pub admin_password: Option<String>,
    pub admin_endpoint: Option<Url>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ConfigToml {
    pub admin: AdminToml,
}

impl ConfigToml {
    pub fn load(data_dir: Option<&Path>) -> Result<Option<Self>> {
        let Some(dir) = data_dir else {
            return Ok(None);
        };

        let config_path = dir.join("config.toml");
        let content = std::fs::read_to_string(&config_path).with_context(|| {
            format!("failed to read config file: {}", config_path.display())
        })?;
        let config = toml::from_str(&content).with_context(|| {
            format!("failed to parse config file: {}", config_path.display())
        })?;
        Ok(Some(config))
    }
}
