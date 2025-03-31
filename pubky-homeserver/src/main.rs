use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pubky_homeserver::HomeserverSuite;
use tracing_subscriber::EnvFilter;

fn default_config_dir_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".pubky")
}

/// Validate that the data_dir path is a directory.
/// It doesnt need to exist, but if it does, it needs to be a directory.
fn validate_config_dir_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.exists() && path.is_file() {
        return Err(format!("Given path is not a directory: {}", path.display()));
    }
    Ok(path)
}

#[derive(Parser, Debug)]
struct Cli {
    /// Path to config file. Defaults to ~/.pubky/config.toml
    #[clap(short, long, default_value_os_t = default_config_dir_path(), value_parser = validate_config_dir_path)]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("pubky_homeserver=debug,tower_http=debug")),
        )
        .init();

    tracing::debug!("Using data dir: {}", args.data_dir.display());

    let _server = HomeserverSuite::run_with_data_dir_path(args.data_dir).await?;

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    Ok(())
}
