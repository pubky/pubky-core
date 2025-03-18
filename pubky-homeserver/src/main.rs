use std::path::PathBuf;

use anyhow::Result;
use pubky_homeserver::Homeserver;

use clap::Parser;

#[derive(Parser, Debug)]
struct Cli {
    /// [tracing_subscriber::EnvFilter]
    #[clap(short, long)]
    tracing_env_filter: Option<String>,

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
        if let Some(config_path) = args.config {
            Homeserver::run_with_config_file(config_path).await?
        } else {
            Homeserver::builder().run().await?
        }
    };

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    server.shutdown().await;

    Ok(())
}
