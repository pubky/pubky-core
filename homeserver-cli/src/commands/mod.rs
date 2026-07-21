mod admin;

use crate::cli::Cli;
use crate::config::ConfigToml;
use clap::Subcommand;
use anyhow::Result;

#[derive(Subcommand, Debug)]
pub enum Commands {
    Admin(admin::AdminCmd),
}

pub fn execute(cli: Cli, config: Option<ConfigToml>) -> Result<()> {
    match cli.command {
        Commands::Admin(cmd) => cmd.run(config)?,
    };
    Ok(())
}
