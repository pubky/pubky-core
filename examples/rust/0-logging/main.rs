use anyhow::Result;
use clap::Parser;
use pubky_testnet::{pubky::Keypair, EphemeralTestnet};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::level_filters::LevelFilter;
use tracing::{debug, info};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(
    version,
    about = "Configure tracing before calling into the Pubky SDK."
)]
struct Cli {
    /// Maximum tracing verbosity to enable: error|warn|info|debug|trace
    #[arg(long, default_value_t = LevelFilter::INFO, value_parser = clap::value_parser!(LevelFilter))]
    level: LevelFilter,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli { level } = Cli::parse();
    init_tracing(level);
    info!(%level, "Tracing initialized");

    info!("Starting ephemeral testnet");
    let testnet = EphemeralTestnet::start().await?;
    let pubky = testnet.sdk()?;
    let homeserver = testnet.homeserver();

    let keypair = Keypair::random();
    let public_key = keypair.public_key().to_string();
    info!(%public_key, "Generated ephemeral signer");

    let signer = pubky.signer(keypair);
    info!(homeserver = %homeserver.public_key(), "Signing up");
    let session = signer.signup(&homeserver.public_key(), None).await?;

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let path = format!("/pub/logging.example/{timestamp}.txt");
    let body = format!("Tracing level {} at {timestamp}", level);
    info!(%path, "Writing sample data");
    session.storage().put(&path, body).await?;

    debug!(%path, "Fetching what we just wrote");
    let fetched = session.storage().get(&path).await?.text().await?;
    info!(%path, %fetched, "Roundtrip complete");

    Ok(())
}

fn init_tracing(level: LevelFilter) {
    let env_filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy();

    fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_level(true)
        .init();
}
