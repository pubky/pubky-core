use std::path::PathBuf;

use anyhow::Result;
use pubky_homeserver::{Homeserver, HomeserverBuilder};
use dirs;
use pubky_homeserver::DataDir;
use clap::Parser;
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
                .unwrap_or_else(|_| EnvFilter::new("pubky_homeserver=debug,tower_http=debug"))
        )
        .init();

    tracing::debug!("Using data dir: {}", args.data_dir.display());

    let data_dir = DataDir::new(args.data_dir);
    data_dir.ensure_data_dir_exists_and_is_writable()?;
    let config = data_dir.read_or_create_config_file()?;
    let keypair = data_dir.read_or_create_keypair()?;
    tracing::debug!("{config:?} {:?}", keypair.public_key());


    let mut builder = HomeserverBuilder::default();
    builder
    .keypair(keypair)
    .admin_password(config.admin_api.admin_password);

    if let Some(domain) = config.icann_drive_api.domain.as_ref() {
        builder.domain(domain.as_str());
    }
    if let Some(boostrap_nodes) = config.pkdns.dht_bootstrap_nodes.as_ref() {
        // builder.bootstrap(boostrap_nodes);
    }




    // let server =  Homeserver::test(boostrap_nodes);

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    // server.shutdown().await;

    Ok(())
}
