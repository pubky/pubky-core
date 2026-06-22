mod cli;
mod commands;
mod config;

use clap::Parser;
use cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Loading and parsing config only if data_dir flag is provided
    let data_dir_flag_provided = std::env::args_os()
        .any(|arg| arg == "--data-dir" || arg == "-d");

    let config = data_dir_flag_provided
        .then(|| config::ConfigToml::load(&cli.data_dir))
        .transpose()?;

    commands::execute(cli, config);
    Ok(())
}
