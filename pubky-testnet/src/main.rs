use anyhow::Result;
use pubky_testnet::FixedTestnet;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "pubky_homeserver=debug,http_relay=debug,pkarr_relay=debug,tower_http=debug,pubky_testnet=debug"
                .to_string(),
        )
        .init();

    let testnet = FixedTestnet::run().await?;
    tracing::info!("Testnet running");
    tracing::info!(
        "DHT Bootstrap Nodes: {}",
        testnet
            .flexible_testnet
            .dht_bootstrap_nodes()
            .into_iter()
            .map(|node| node.to_string())
            .collect::<Vec<String>>()
            .join(", ")
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
