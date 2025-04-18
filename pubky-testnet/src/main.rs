use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pubky_testnet::StaticTestnet;

#[derive(Parser, Debug)]
struct Cli {
    /// Optional path to a homeserver config file. This overrides the default config.
    #[clap(long)]
    homeserver_config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            "pubky_homeserver=debug,http_relay=debug,pkarr_relay=info,tower_http=debug,pubky_testnet=debug"
                .to_string(),
        )
        .init();

    let testnet = if let Some(config_path) = args.homeserver_config {
        StaticTestnet::start_with_homeserver_config(config_path).await?
    } else {
        StaticTestnet::start().await?
    };

    tracing::info!("Testnet running");
    tracing::info!(
        "DHT Bootstrap Nodes: {}",
        testnet.bootstrap_nodes().join(", ")
    );
    tracing::info!("Pkarr Relay: {}", testnet.pkarr_relay().local_url());
    tracing::info!("Http Relay: {}", testnet.http_relay().local_url());
    tracing::info!(
        "Homeserver ICANN HTTP: {}",
        testnet.homeserver_suite().icann_http_url()
    );
    tracing::info!(
        "Homeserver Pubky HTTPS: {}",
        testnet.homeserver_suite().pubky_url()
    );
    tracing::info!(
        "Homeserver admin: http://{}",
        testnet.homeserver_suite().admin().listen_socket()
    );

    tokio::signal::ctrl_c().await?;

    Ok(())
}
