use crate::config::ConfigToml;
use clap::{Args, Subcommand};
use url::Url;
mod context;
pub mod generate_signup_token;
pub mod get_info;
use context::AdminContext;

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
    GenerateSignupToken(generate_signup_token::GenerateSignupTokenArgs),
}

impl AdminCmd {
    pub fn run(&self, config: Option<ConfigToml>) -> anyhow::Result<()> {
        let settings = AdminContext::resolve(self, config.as_ref())?;

        match &self.subcommand {
            AdminSubcommands::GetInfo(sbu_args) => {
                get_info::run(settings, sbu_args)?;
            }
            AdminSubcommands::GenerateSignupToken(sbu_args) => {
                generate_signup_token::run(settings, sbu_args)?;
            }
        }
        Ok(())
    }
}
