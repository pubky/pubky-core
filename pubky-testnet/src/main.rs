use anyhow::Result;
use pubky_testnet::Testnet;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            "pubky_homeserver=debug,http_relay=debug,pkarr_relay=debug,tower_http=debug"
                .to_string(),
        )
        .init();

    Testnet::run_with_hardcoded_configurations().await?;

    tokio::signal::ctrl_c().await?;

    Ok(())
}
