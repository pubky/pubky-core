use serde::{Deserialize, Serialize};
use std::path::Path;
use url::Url;

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
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let config_path = data_dir.join("config.toml");
        let content = std::fs::read_to_string(&config_path)?;
        let config: ConfigToml = toml::from_str(&content)?;
        Ok(config)
    }
}
