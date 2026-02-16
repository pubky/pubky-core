use super::AppState;

#[cfg(any(test, feature = "testing"))]
use crate::MockDataDir;

use crate::{
    app_context::{AppContext, AppContextConversionError},
    PersistentDataDir,
};
use anyhow::Result;
use futures_util::TryFutureExt;
use pubky_common::auth::AuthVerifier;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use axum::{
    routing::{get, post},
    Router,
};
use axum_server::{
    tls_rustls::{RustlsAcceptor, RustlsConfig},
    Handle,
};
use std::{net::SocketAddr, sync::Arc};
use tower_cookies::CookieManagerLayer;
use tower_http::cors::CorsLayer;

use super::layers::{
    pubky_host::PubkyHostLayer, rate_limiter::RateLimiterLayer, trace::with_trace_layer,
};
use super::routes::{auth, events, root, tenants};

/// Errors that can occur when building a `HomeserverCore`.
#[derive(Debug, thiserror::Error)]
pub enum ClientServerBuildError {
    /// Failed to run the ICANN web server.
    #[error("ICANN web server error: {0}")]
    IcannWebServer(anyhow::Error),
    /// Failed to run the Pubky TLS web server.
    #[error("Pubky TLS web server error: {0}")]
    PubkyTlsServer(anyhow::Error),
    /// Failed to convert the data directory to an AppContext.
    #[error("AppContext conversion error: {0}")]
    AppContext(AppContextConversionError),
}

impl From<AppContextConversionError> for ClientServerBuildError {
    fn from(error: AppContextConversionError) -> Self {
        ClientServerBuildError::AppContext(error)
    }
}

/// A Pubky homeserver with ICANN HTTP and Pubky TLS servers.
pub struct ClientServer {
    /// Keep context alive.
    context: AppContext,

    pub(crate) icann_http_handle: Handle<SocketAddr>,
    pub(crate) icann_http_socket: SocketAddr,

    pub(crate) pubky_tls_handle: Handle<SocketAddr>,
    pub(crate) pubky_tls_socket: SocketAddr,
}

impl ClientServer {
    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir_path(
        dir_path: PathBuf,
    ) -> Result<Self, ClientServerBuildError> {
        let data_dir = PersistentDataDir::new(dir_path);
        let context = AppContext::read_from(data_dir).await?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir(
        dir: PersistentDataDir,
    ) -> Result<Self, ClientServerBuildError> {
        let context = AppContext::read_from(dir).await?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory mock.
    #[cfg(any(test, feature = "testing"))]
    pub async fn start_with_mock_data_dir(
        dir: MockDataDir,
    ) -> Result<Self, ClientServerBuildError> {
        let context = AppContext::read_from(dir).await?;
        Self::start(context).await
    }

    /// Start homeserver services with the given application context.
    pub async fn start(context: AppContext) -> std::result::Result<Self, ClientServerBuildError> {
        let router = Self::create_router(&context);

        let (icann_http_handle, icann_http_socket) =
            Self::start_icann_http_server(&context, router.clone())
                .await
                .map_err(ClientServerBuildError::IcannWebServer)?;
        let (pubky_tls_handle, pubky_tls_socket) = Self::start_pubky_tls_server(&context, router)
            .await
            .map_err(ClientServerBuildError::PubkyTlsServer)?;

        Ok(Self {
            context,
            icann_http_handle,
            pubky_tls_handle,
            icann_http_socket,
            pubky_tls_socket,
        })
    }

    pub(crate) fn create_router(context: &AppContext) -> Router {
        let quota_mb = context.config_toml.general.user_storage_quota_mb;
        let quota_bytes = if quota_mb == 0 {
            None
        } else {
            Some(quota_mb * 1024 * 1024)
        };

        let state = AppState {
            verifier: AuthVerifier::default(),
            sql_db: context.sql_db.clone(),
            file_service: context.file_service.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
            user_quota_bytes: quota_bytes,
            metrics: context.metrics.clone(),
            events_service: context.events_service.clone(),
        };
        super::create_app(state.clone(), context)
    }

    /// Start the ICANN HTTP server
    async fn start_icann_http_server(
        context: &AppContext,
        router: Router,
    ) -> Result<(Handle<SocketAddr>, SocketAddr)> {
        // Icann http server
        let http_listener = TcpListener::bind(context.config_toml.drive.icann_listen_socket)?;
        let http_socket = http_listener.local_addr()?;
        let http_handle = Handle::new();
        let server = axum_server::from_tcp(http_listener)?;
        tokio::spawn(
            server
                .handle(http_handle.clone())
                .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                .map_err(|error| {
                    tracing::error!(?error, "Homeserver icann http server error");
                    println!("Homeserver icann http server error: {:?}", error);
                }),
        );

        Ok((http_handle, http_socket))
    }

    /// Start the Pubky TLS server
    async fn start_pubky_tls_server(
        context: &AppContext,
        router: Router,
    ) -> Result<(Handle<SocketAddr>, SocketAddr)> {
        // Pubky tls server
        let https_listener = TcpListener::bind(context.config_toml.drive.pubky_listen_socket)?;
        let https_socket = https_listener.local_addr()?;
        let https_handle = Handle::new();
        let server = axum_server::from_tcp(https_listener)?;
        tokio::spawn(
            server
                .acceptor(RustlsAcceptor::new(RustlsConfig::from_config(Arc::new(
                    context.keypair.to_rpk_rustls_server_config(),
                ))))
                .handle(https_handle.clone())
                .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                .map_err(|error| {
                    tracing::error!(?error, "Homeserver pubky tls server error");
                    println!("Homeserver pubky tls server error: {:?}", error);
                }),
        );

        Ok((https_handle, https_socket))
    }
    /// Get the URL of the icann http server.
    pub fn icann_http_url_string(&self) -> String {
        format!("http://{}", self.icann_http_socket)
    }

    /// Get the URL of the pubky tls server with the Pubky DNS name.
    pub fn pubky_tls_dns_url_string(&self) -> String {
        format!("https://{}", self.context.keypair.public_key().z32())
    }

    /// Get the URL of the pubky tls server with the Pubky IP address.
    pub fn pubky_tls_ip_url_ring(&self) -> String {
        format!("https://{}", self.pubky_tls_socket)
    }

    /// Shutdown the http and tls servers.
    pub fn shutdown(&self) {
        self.icann_http_handle
            .graceful_shutdown(Some(Duration::from_secs(5)));
        self.pubky_tls_handle
            .graceful_shutdown(Some(Duration::from_secs(5)));
    }
}

impl Drop for ClientServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn base() -> Router<AppState> {
    Router::new()
        .route("/", get(root::handler))
        .route("/signup", post(auth::signup))
        .route("/session", post(auth::signin))
        // Events
        .route("/events/", get(events::feed))
        .route("/events-stream", get(events::feed_stream))

    // TODO: add size limit
    // TODO: revisit if we enable streaming big payloads
    // TODO: maybe add to a separate router (drive router?).
}

pub fn create_app(state: AppState, context: &AppContext) -> Router {
    let app = base()
        .merge(tenants::router(state.clone()))
        .layer(CookieManagerLayer::new())
        .layer(CorsLayer::very_permissive())
        .layer(RateLimiterLayer::new(
            context.config_toml.drive.rate_limits.clone(),
        ))
        .layer(PubkyHostLayer)
        .with_state(state);

    // Apply trace and pubky host layers to the complete router.
    with_trace_layer(app)
}
