use clap::{Args, Subcommand};
use crate::config::ConfigToml;
use url::Url;
pub mod get_info;
use anyhow::{Context, Result};

pub trait ResolveAdminFlags {
    fn resolve_admin_password(&self, config: Option<&ConfigToml>) -> Result<String>;
    fn resolve_admin_endpoint(&self, config: Option<&ConfigToml>) -> Result<Url>;
}

impl ResolveAdminFlags for AdminCmd {
    fn resolve_admin_password(&self, config: Option<&ConfigToml>) -> Result<String> {
        match &self.admin_password {
            Some(Some(password)) => Ok(password.clone()),
            Some(None) => rpassword::prompt_password("Provide Homeserver Admin Password: ")
                .context("Failed to read admin password from terminal"),
            None => config
                .and_then(|c| c.admin.admin_password.clone())
                .context("Missing admin password. Provide it via '--admin-password' or in the config file."),
        }
    }

    fn resolve_admin_endpoint(&self, config: Option<&ConfigToml>) -> Result<Url> {
        self.admin_endpoint
            .clone()
            .or_else(|| config.and_then(|c| c.admin.admin_endpoint.clone()))
            .context("Error: Missing admin endpoint. Provide it via '--admin-endpoint' or in the config file.")
    }
}

#[derive(Args, Debug)]
pub struct AdminCmd {
    #[arg(long, num_args = 0..=1)]
    pub admin_password: Option<Option<String>>,

    #[arg(long)]
    pub admin_endpoint: Option<Url>,

    #[command(subcommand)]
    pub subcommand: AdminSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum AdminSubcommands {
    GetInfo(get_info::GetInfoArgs),
}

impl AdminCmd {
    pub fn run(&self, config: Option<ConfigToml>) -> anyhow::Result<()> {
        let resolved_admin_password = self.resolve_admin_password(config.as_ref())?;
        let resolved_admin_endpoint = self.resolve_admin_endpoint(config.as_ref())?;

        match &self.subcommand {
            AdminSubcommands::GetInfo(sbu_args) => {
                get_info::run(resolved_admin_endpoint, resolved_admin_password, sbu_args)?;
            }
        }
        Ok(())
    }
}
