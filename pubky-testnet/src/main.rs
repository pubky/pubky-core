use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Parser, Subcommand};
use pubky_testnet::StaticTestnet;

#[derive(Parser, Debug)]
struct Cli {
    /// Optional path to a homeserver config file.
    /// In in-memory mode (default), this overrides the default config.
    /// With `persist`, this writes the initial config.toml on first run.
    #[clap(long)]
    homeserver_config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a persistent testnet with state stored in the given data directory.
    Persist {
        /// Path to the data directory (config, keypair, files).
        /// Created automatically on first run.
        data_dir: PathBuf,
    },
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

    let mut builder = StaticTestnet::builder();
    if let Some(config) = args.homeserver_config {
        builder = builder.homeserver_config(config);
    }
    if let Some(Command::Persist { data_dir }) = args.command {
        tracing::info!(
            "Persistent testnet configured. data dir: {}",
            data_dir.display()
        );
        builder = builder.persistent(data_dir);
    }

    let testnet = builder.build().await?;
    tracing::info!("Testnet running");
    tracing::info!(
        "DHT Bootstrap Nodes: {}",
        testnet.bootstrap_nodes().join(", ")
    );
    tracing::info!("Pkarr Relay: {}", testnet.pkarr_relay().local_url());
    tracing::info!("Http Relay: {}", testnet.http_relay().local_url());
    tracing::info!(
        "Homeserver ICANN HTTP: {}",
        testnet.homeserver_app().icann_http_url()
    );
    tracing::info!(
        "Homeserver Pubky HTTPS: {}",
        testnet.homeserver_app().pubky_url()
    );
    if let Some(admin_server) = testnet.homeserver_app().admin_server() {
        tracing::info!("Homeserver admin: http://{}", admin_server.listen_socket());
    }
    if let Some(metrics_server) = testnet.homeserver_app().metrics_server() {
        tracing::info!(
            "Homeserver metrics: http://{}",
            metrics_server.listen_socket()
        );
    }

    tokio::signal::ctrl_c().await?;
    let persistent = testnet.is_persistent();
    drop(testnet);

    if !persistent {
        cleanup_ephemeral_databases().await;
    }

    Ok(())
}

/// Drops the ephemeral test databases the testnet created.
///
/// A database is only registered for the drop once the last handle to it is
/// released, and the background tasks holding those handles (http servers,
/// republishers) shut down asynchronously — so registrations trickle in after
/// the testnet value itself has been dropped rather than all at once.
///
/// Wait for the first one, then keep draining until nothing new has registered
/// for [`QUIET_PERIOD`], so a late arrival is not left behind on the server.
async fn cleanup_ephemeral_databases() {
    if !wait_for_db_registration(DB_REGISTRATION_TIMEOUT).await {
        tracing::warn!(
            "Timed out waiting for the ephemeral database to be released; \
             it may be left behind on the postgres server."
        );
        return;
    }

    loop {
        pubky_testnet::drop_test_databases().await;
        // Anything that registers during this window is picked up by the next
        // pass; a full quiet period with nothing new means we are done.
        if !wait_for_db_registration(QUIET_PERIOD).await {
            return;
        }
    }
}

/// How long to wait for the first ephemeral database to be registered for cleanup.
/// Generous enough to cover the graceful shutdown of the http servers.
const DB_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a drain pass waits for stragglers before declaring cleanup finished.
const QUIET_PERIOD: Duration = Duration::from_millis(500);

/// How often to re-check the registration list.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Waits until at least one ephemeral test database is registered for cleanup.
///
/// Returns `true` if one showed up, `false` if `timeout` elapsed first.
async fn wait_for_db_registration(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !pubky_testnet::has_registered_dbs() {
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    true
}
