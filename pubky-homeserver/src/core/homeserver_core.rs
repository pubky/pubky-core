use std::path::PathBuf;
use std::time::Duration;

use super::key_republisher::HomeserverKeyRepublisher;
use super::periodic_backup::PeriodicBackup;
use super::sessions::{JwtService, SessionManager};
use crate::app_context::AppContextConversionError;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use crate::persistence::lmdb::LmDB;
use crate::{app_context::AppContext, PersistentDataDir};
use crate::{DataDir, MockDataDir, SignupMode};
use anyhow::Result;
use axum::Router;
use axum_server::{
    tls_rustls::{RustlsAcceptor, RustlsConfig},
    Handle,
};
use futures_util::TryFutureExt;
use pubky_common::auth::AuthVerifier;
use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    pub(crate) db: LmDB,
    pub(crate) signup_mode: SignupMode,
    pub(crate) session_manager: SessionManager,
    /// If `Some(bytes)` the quota is enforced, else unlimited.
    pub(crate) user_quota_bytes: Option<u64>,
}

const INITIAL_DELAY_BEFORE_REPUBLISH: Duration = Duration::from_secs(60);

/// Errors that can occur when building a `HomeserverCore`.
#[derive(Debug, thiserror::Error)]
pub enum HomeserverBuildError {
    /// Failed to run the key republisher.
    #[error("Key republisher error: {0}")]
    KeyRepublisher(anyhow::Error),
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

/// A side-effect-free Core of the [crate::Homeserver].
pub struct HomeserverCore {
    #[allow(dead_code)]
    // Keep this alive. Republishing is stopped when the UserKeysRepublisher is dropped.
    pub(crate) user_keys_republisher: UserKeysRepublisher,
    #[allow(dead_code)]
    // Keep this alive. Republishing is stopped when the HomeserverKeyRepublisher is dropped.
    pub(crate) key_republisher: HomeserverKeyRepublisher,
    #[allow(dead_code)] // Keep this alive. Backup is stopped when the PeriodicBackup is dropped.
    pub(crate) periodic_backup: PeriodicBackup,
    /// Keep context alive.
    context: AppContext,
    pub(crate) icann_http_handle: Handle,
    pub(crate) pubky_tls_handle: Handle,
    pub(crate) icann_http_socket: SocketAddr,
    pub(crate) pubky_tls_socket: SocketAddr,
}

impl HomeserverCore {
    /// Create a Homeserver from a data directory path like `~/.pubky`.
    pub async fn from_persistent_data_dir_path(
        dir_path: PathBuf,
    ) -> std::result::Result<Self, HomeserverBuildError> {
        let data_dir = PersistentDataDir::new(dir_path);
        Self::from_persistent_data_dir(data_dir).await
    }

    /// Create a Homeserver from a data directory.
    pub async fn from_persistent_data_dir(
        data_dir: PersistentDataDir,
    ) -> std::result::Result<Self, HomeserverBuildError> {
        Self::from_data_dir(Arc::new(data_dir)).await
    }

    /// Create a Homeserver from a mock data directory.
    pub async fn from_mock_data_dir(
        mock_dir: MockDataDir,
    ) -> std::result::Result<Self, HomeserverBuildError> {
        Self::from_data_dir(Arc::new(mock_dir)).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub(crate) async fn from_data_dir(
        dir: Arc<dyn DataDir>,
    ) -> std::result::Result<Self, HomeserverBuildError> {
        let context = AppContext::try_from(dir).map_err(HomeserverBuildError::AppContext)?;
        Self::new(context).await
    }

    /// Create a Homeserver from an AppContext.
    /// - Publishes the homeserver's pkarr packet to the DHT.
    /// - (Optional) Publishes the user's keys to the DHT.
    /// - (Optional) Runs a periodic backup of the database.
    /// - Creates the web server (router) for testing. Use `listen` to start the server.
    pub async fn new(context: AppContext) -> std::result::Result<Self, HomeserverBuildError> {
        let router = Self::create_router(&context);

        let (icann_http_handle, icann_http_socket) =
            Self::start_icann_http_server(&context, router.clone())
                .await
                .map_err(HomeserverBuildError::IcannWebServer)?;
        let (pubky_tls_handle, pubky_tls_socket) = Self::start_pubky_tls_server(&context, router)
            .await
            .map_err(HomeserverBuildError::PubkyTlsServer)?;

        let key_republisher = HomeserverKeyRepublisher::start(
            &context,
            icann_http_socket.port(),
            pubky_tls_socket.port(),
        )
        .await
        .map_err(HomeserverBuildError::KeyRepublisher)?;
        let user_keys_republisher =
            UserKeysRepublisher::start_delayed(&context, INITIAL_DELAY_BEFORE_REPUBLISH);
        let periodic_backup = PeriodicBackup::start(&context);

        Ok(Self {
            user_keys_republisher,
            key_republisher,
            periodic_backup,
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
            db: context.db.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
            user_quota_bytes: quota_bytes,
            session_manager: SessionManager::new(context),
        };
        super::routes::create_app(state.clone())
    }

    /// Start the ICANN HTTP server
    async fn start_icann_http_server(
        context: &AppContext,
        router: Router,
    ) -> Result<(Handle, SocketAddr)> {
        // Icann http server
        let http_listener = TcpListener::bind(context.config_toml.drive.icann_listen_socket)?;
        let http_socket = http_listener.local_addr()?;
        let http_handle = Handle::new();
        tokio::spawn(
            axum_server::from_tcp(http_listener)
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
    ) -> Result<(Handle, SocketAddr)> {
        // Pubky tls server
        let https_listener = TcpListener::bind(context.config_toml.drive.pubky_listen_socket)?;
        let https_socket = https_listener.local_addr()?;
        let https_handle = Handle::new();
        tokio::spawn(
            axum_server::from_tcp(https_listener)
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
    pub fn icann_http_url(&self) -> String {
        format!("http://{}", self.icann_http_socket)
    }

    /// Get the URL of the pubky tls server with the Pubky DNS name.
    pub fn pubky_tls_dns_url(&self) -> String {
        format!("https://{}", self.context.keypair.public_key())
    }

    /// Get the URL of the pubky tls server with the Pubky IP address.
    pub fn pubky_tls_ip_url(&self) -> String {
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

impl Drop for HomeserverCore {
    fn drop(&mut self) {
        self.shutdown();
    }
}
