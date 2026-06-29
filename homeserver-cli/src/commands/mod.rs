mod admin;

use crate::cli::Cli;
use crate::config::ConfigToml;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Commands {
    Admin(admin::AdminCmd),
}

pub fn execute(cli: Cli, _config: Option<ConfigToml>) {
    match cli.command {
        Commands::Admin(cmd) => cmd.run(_config),
    }
}
