use std::path::PathBuf;

use anyhow::Result;
use pubky_homeserver::{Config, Homeserver};

use clap::Parser;

#[derive(Parser, Debug)]
struct Cli {
    /// [tracing_subscriber::EnvFilter]
    #[clap(short, long)]
    tracing_env_filter: Option<String>,

    /// Run Homeserver in a local testnet
    #[clap(long)]
    testnet: bool,

    /// Optional Path to config file.
    #[clap(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            args.tracing_env_filter
                .unwrap_or("pubky_homeserver=debug,tower_http=debug".to_string()),
        )
        .init();

    let server = unsafe {
        if args.testnet {
            Homeserver::start_testnet().await?
        } else if let Some(config_path) = args.config {
            Homeserver::start(Config::load(config_path).await?).await?
        } else {
            Homeserver::builder().build().await?
        }
    };

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    server.shutdown();

    Ok(())
}
