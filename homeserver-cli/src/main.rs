mod cli;
mod commands;
mod config;

use clap::Parser;
use cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = config::ConfigToml::load(cli.data_dir.as_deref())?;

    commands::execute(cli, config)?;
    Ok(())
}
