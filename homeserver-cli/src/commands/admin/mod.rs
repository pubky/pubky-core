use clap::{Args, Subcommand};
use crate::config::ConfigToml;
use url::Url;
pub mod get_info;

pub trait ResolveAdminFlags {
    fn resolve_admin_password(&self, config: Option<&ConfigToml>) -> Result<String, String>;
    fn resolve_admin_endpoint(&self, config: Option<&ConfigToml>) -> Result<Url, String>;
}

//admin_passowrd and admin_endpoint could be provided via config or command flags, the flags. The flags have higher priority.
impl ResolveAdminFlags for AdminCmd {
    fn resolve_admin_password(&self, config: Option<&ConfigToml>) -> Result<String, String> {
        self.admin_password.clone().or_else(|| {
            config.and_then(|c| c.admin.admin_password.clone())
        })
        .ok_or_else(|| "Error: Missing admin password. Provide it via '--admin-password' or in the config file.".to_string())
    }

    fn resolve_admin_endpoint(&self, config: Option<&ConfigToml>) -> Result<Url, String> {
        self.admin_endpoint.clone().or_else(|| {
            config.and_then(|c| c.admin.admin_endpoint.clone())
        })
        .ok_or_else(|| "Error: Missing admin endpoint. Provide it via '--admin-endpoint' or in the config file.".to_string())
    }
}

#[derive(Args, Debug)]
pub struct AdminCmd {
    #[arg(long)]
    pub admin_password: Option<String>,

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
    pub fn run(&self, config: Option<ConfigToml>) {
        let resolved_admin_password = match self.resolve_admin_password(config.as_ref()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        };
        let resolved_admin_endpoint = match self.resolve_admin_endpoint(config.as_ref()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        };

        match &self.subcommand {
            AdminSubcommands::GetInfo(sbu_args) => {
                get_info::run(resolved_admin_endpoint, resolved_admin_password, sbu_args);
            }
        }
    }
}