use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pubky_homeserver::{tracing::init_tracing_logs_if_set, HomeserverSuite};

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
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Path to config file. Defaults to ~/.pubky/config.toml
    #[clap(short, long, default_value_os_t = default_config_dir_path(), value_parser = validate_config_dir_path)]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    init_tracing_logs_if_set(&args.data_dir)?;

    tracing::info!("Use data directory: {}", args.data_dir.display());
    let server = HomeserverSuite::start_with_persistent_data_dir_path(args.data_dir).await?;

    tracing::info!(
        "Homeserver HTTP listening on {}",
        server.core().icann_http_url()
    );

    tracing::info!(
        "Homeserver Pubky TLS listening on {}",
        server.core().pubky_tls_dns_url(),
    );
    tracing::info!(
        "Homeserver Pubky TLS listening on {}",
        server.core().pubky_tls_ip_url()
    );
    tracing::info!(
        "Admin server listening on http://{}",
        server.admin().listen_socket()
    );

    tracing::info!("Press Ctrl+C to stop the Homeserver");
    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    Ok(())
}
