use crate::config::ConfigToml;
use clap::{Args, Subcommand};
use url::Url;
mod context;
pub mod error;
pub mod generate_signup_token;
pub mod get_info;
pub mod quota;
pub mod user;
use context::AdminContext;

#[derive(Args, Debug)]
pub struct AdminCmd {
    #[arg(long)]
    pub admin_password: bool,

    #[arg(long)]
    pub admin_endpoint: Option<Url>,

    #[command(subcommand)]
    pub subcommand: AdminSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum AdminSubcommands {
    GetInfo(get_info::GetInfoArgs),
    GenerateSignupToken(generate_signup_token::GenerateSignupTokenArgs),
    User(user::UserCmd),
    Quota(quota::QuotaCmd),
}

impl AdminCmd {
    pub fn run(&self, config: Option<ConfigToml>) -> anyhow::Result<()> {
        let context = AdminContext::resolve(self, config.as_ref())?;

        match &self.subcommand {
            AdminSubcommands::GetInfo(sbu_args) => {
                get_info::run(context, sbu_args)?;
            }
            AdminSubcommands::GenerateSignupToken(sbu_args) => {
                generate_signup_token::run(context, sbu_args)?;
            }
            AdminSubcommands::User(sbu_args) => {
                sbu_args.run(context)?;
            }
            AdminSubcommands::Quota(sbu_args) => {
                sbu_args.run(context)?;
            }
        }
        Ok(())
    }
}
