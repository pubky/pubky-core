use anyhow::{Context, Result};
use url::Url;

use super::AdminCmd;
use crate::config::ConfigToml;

pub struct AdminSettings {
    pub password: String,
    pub endpoint: Url,
}

impl AdminSettings {
    pub fn resolve(cmd: &AdminCmd, config: Option<&ConfigToml>) -> Result<Self> {
        Ok(Self {
            password: resolve_password(cmd, config)?,
            endpoint: resolve_endpoint(cmd, config)?,
        })
    }
}

fn resolve_password(cmd: &AdminCmd, config: Option<&ConfigToml>) -> Result<String> {
    match &cmd.admin_password {
        Some(Some(password)) => Ok(password.clone()),
        Some(None) => rpassword::prompt_password("Provide Homeserver Admin Password: ")
            .context("Failed to read admin password from terminal"),
        None => config
            .and_then(|c| c.admin.admin_password.clone())
            .context("Missing admin password. Provide it via '--admin-password' or in the config file."),
    }
}

fn resolve_endpoint(cmd: &AdminCmd, config: Option<&ConfigToml>) -> Result<Url> {
    cmd.admin_endpoint
        .clone()
        .or_else(|| config.and_then(|c| c.admin.admin_endpoint.clone()))
        .context("Missing admin endpoint. Provide it via '--admin-endpoint' or in the config file.")
}
