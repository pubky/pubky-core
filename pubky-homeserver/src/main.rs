use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pubky_homeserver::{tracing::init_tracing, DataDir, HomeserverApp, PersistentDataDir};

fn default_data_dir_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".pubky")
}

/// Validate that a path is not an existing *file* (directories and non-existent paths are fine).
fn validate_dir_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.exists() && path.is_file() {
        return Err(format!(
            "Expected a directory, got a file: {}",
            path.display()
        ));
    }
    Ok(path)
}

/// Validate that a path is not an existing *directory* (files and non-existent paths are fine).
fn validate_file_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.exists() && path.is_dir() {
        return Err(format!(
            "Expected a file, got a directory: {}",
            path.display()
        ));
    }
    Ok(path)
}

#[derive(Parser, Debug)]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Path to config.toml.
    /// Defaults to <data-dir>/config.toml.
    #[clap(short = 'c', long, value_parser = validate_file_path)]
    config: Option<PathBuf>,

    /// Directory for the secret key and derived defaults.
    /// Config and secret key paths fall back to <data-dir>/config.toml and <data-dir>/secret
    /// when not set explicitly.
    #[clap(short = 'd', long, default_value_os_t = default_data_dir_path(), value_parser = validate_dir_path)]
    data_dir: PathBuf,

    /// Path to the secret key file.
    /// Defaults to <data-dir>/secret.
    #[clap(short = 'k', long, value_parser = validate_file_path)]
    secret_key: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let homeserver_paths =
        PersistentDataDir::new_with_overrides(args.data_dir, args.config, args.secret_key);

    // Initialize tracing early so that config-loading errors are captured.
    init_tracing(&homeserver_paths)?;

    tracing::info!(
        "Using data directory: {}",
        homeserver_paths.path().display()
    );
    tracing::info!(
        "Using config file: {}",
        homeserver_paths.config_file_path().display()
    );
    tracing::info!(
        "Using secret key file: {}",
        homeserver_paths.secret_file_path().display()
    );

    let server = HomeserverApp::start_with_persistent_data_dir(homeserver_paths).await?;

    tracing::info!(
        "Homeserver HTTP listening on {}",
        server.client_server().icann_http_url_string()
    );

    tracing::info!(
        "Homeserver Pubky TLS listening on {}",
        server.client_server().pubky_tls_dns_url_string(),
    );
    tracing::info!(
        "Homeserver Pubky TLS listening on {}",
        server.client_server().pubky_tls_ip_url_ring()
    );
    if let Some(admin_server) = server.admin_server() {
        tracing::info!(
            "Admin server listening on http://{}",
            admin_server.listen_socket()
        );
    }
    if let Some(metrics_server) = server.metrics_server() {
        tracing::info!(
            "Metrics server listening on http://{}",
            metrics_server.listen_socket()
        );
    }

    tracing::info!("Press Ctrl+C to stop the Homeserver");
    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    Ok(())
}
