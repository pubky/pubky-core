use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use crate::metrics_server::routes::metrics::Metrics;
use crate::AppContext;
use crate::{AppContextConversionError, PersistentDataDir};
use axum::routing::get;
use axum::Router;
use axum_server::Handle;
use tokio::task::JoinHandle;

fn create_app(metrics: Metrics) -> axum::routing::IntoMakeService<Router> {
    Router::new()
        .route("/metrics", get(metrics.render()))
        .with_state(metrics)
        .into_make_service()
}

/// Errors that can occur when building a `MetricsServer`.
#[derive(thiserror::Error, Debug)]
pub enum MetricsServerBuildError {
    /// Failed to create metrics server.
    #[error("Failed to create metrics server: {0}")]
    Server(anyhow::Error),

    /// Failed to bootstrap from the data directory.
    #[error("Failed to bootstrap from the data directory: {0}")]
    DataDir(AppContextConversionError),
}

/// Metrics server
///
/// This server exposes Prometheus metrics on a separate port.
/// It should be isolated from the public network and only accessible to monitoring systems.
///
/// When dropped, the server will stop.
pub struct MetricsServer {
    http_handle: Handle,
    join_handle: JoinHandle<()>,
    socket: SocketAddr,
}

impl MetricsServer {
    /// Start the metrics server from a persistent data directory.
    pub async fn from_data_dir(
        data_dir: PersistentDataDir,
    ) -> Result<Self, MetricsServerBuildError> {
        let context = AppContext::read_from(data_dir)
            .await
            .map_err(MetricsServerBuildError::DataDir)?;
        Self::start(&context).await
    }

    /// Start the metrics server from a data directory path.
    pub async fn from_data_dir_path(
        data_dir_path: PathBuf,
    ) -> Result<Self, MetricsServerBuildError> {
        let data_dir = PersistentDataDir::new(data_dir_path);
        Self::from_data_dir(data_dir).await
    }

    /// Run the metrics server.
    pub async fn start(context: &AppContext) -> Result<Self, MetricsServerBuildError> {
        let metrics = context.metrics.clone();
        let socket = context
            .config_toml
            .metrics
            .as_ref()
            .ok_or_else(|| {
                MetricsServerBuildError::Server(anyhow::anyhow!("Metrics configuration not found"))
            })?
            .listen_socket;
        let app = create_app(metrics);
        let listener = std::net::TcpListener::bind(socket)
            .map_err(|e| MetricsServerBuildError::Server(e.into()))?;
        let socket = listener
            .local_addr()
            .map_err(|e| MetricsServerBuildError::Server(e.into()))?;
        let http_handle = Handle::new();
        let inner_http_handle = http_handle.clone();
        let join_handle = tokio::spawn(async move {
            axum_server::from_tcp(listener)
                .handle(inner_http_handle)
                .serve(app)
                .await
                .unwrap_or_else(|e| tracing::error!("Metrics server error: {}", e));
        });
        Ok(Self {
            http_handle,
            socket,
            join_handle,
        })
    }

    /// Get the socket address the metrics server is listening on.
    pub fn listen_socket(&self) -> SocketAddr {
        self.socket
    }
}

impl Drop for MetricsServer {
    fn drop(&mut self) {
        self.http_handle
            .graceful_shutdown(Some(Duration::from_secs(5)));
        self.join_handle.abort();
    }
}
