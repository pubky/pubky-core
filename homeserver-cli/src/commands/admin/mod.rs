use clap::{Args, Subcommand};
use crate::config::ConfigToml;
use url::Url;
pub mod get_info;
mod settings;
use settings::AdminSettings;

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
        let settings = AdminSettings::resolve(self, config.as_ref())?;

        match &self.subcommand {
            AdminSubcommands::GetInfo(sbu_args) => {
                get_info::run(settings, sbu_args)?;
            }
        }
        Ok(())
    }
}
