use std::path::PathBuf;
use std::time::Duration;

use super::key_republisher::HomeserverKeyRepublisher;
use super::periodic_backup::PeriodicBackup;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use crate::persistence::lmdb::LmDB;
use crate::{app_context::AppContext, DataDir};
use crate::{DataDirMock, DataDirTrait, SignupMode};
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
}

const INITIAL_DELAY_BEFORE_REPUBLISH: Duration = Duration::from_secs(60);

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
    pub async fn from_data_dir_path(dir_path: PathBuf) -> Result<Self> {
        let data_dir = DataDir::new(dir_path);
        Self::from_data_dir(data_dir).await
    }

    /// Create a Homeserver from a data directory.
    pub async fn from_data_dir(data_dir: DataDir) -> Result<Self> {
        Self::from_data_dir_trait(Arc::new(data_dir)).await
    }

    /// Create a Homeserver from a mock data directory.
    pub async fn from_mock_dir(mock_dir: DataDirMock) -> Result<Self> {
        Self::from_data_dir_trait(Arc::new(mock_dir)).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub(crate) async fn from_data_dir_trait(dir: Arc<dyn DataDirTrait>) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::new(context).await
    }

    /// Create a Homeserver from an AppContext.
    /// - Publishes the homeserver's pkarr packet to the DHT.
    /// - (Optional) Publishes the user's keys to the DHT.
    /// - (Optional) Runs a periodic backup of the database.
    /// - Creates the web server (router) for testing. Use `listen` to start the server.
    pub async fn new(context: AppContext) -> Result<Self> {
        let key_republisher = HomeserverKeyRepublisher::run(&context).await?;
        let user_keys_republisher =
            UserKeysRepublisher::run_delayed(&context, INITIAL_DELAY_BEFORE_REPUBLISH);
        let periodic_backup = PeriodicBackup::run(&context);
        let (icann_http_handle, pubky_tls_handle, icann_http_socket, pubky_tls_socket) = Self::start_server_tasks(&context).await?;
        Ok(Self {
            user_keys_republisher,
            key_republisher,
            periodic_backup,
            context: context.clone(),
            icann_http_handle,
            pubky_tls_handle,
            icann_http_socket,
            pubky_tls_socket,
        })
    }

    pub(crate) fn create_router(context: &AppContext) -> Router {
        let state = AppState {
            verifier: AuthVerifier::default(),
            db: context.db.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        };
        super::routes::create_app(state.clone())
    }

    /// Start listening on the http and tls sockets.
    async fn start_server_tasks(context: &AppContext) -> Result<(Handle, Handle, SocketAddr, SocketAddr)> {
        let router = Self::create_router(context);

        // Icann http server
        let http_listener = TcpListener::bind(context.config_toml.drive.icann_listen_socket)?;
        let http_socket = http_listener.local_addr()?;
        let http_handle = Handle::new();
        tokio::spawn(
            axum_server::from_tcp(http_listener)
                .handle(http_handle.clone())
                .serve(
                    router
                        .clone()
                        .into_make_service_with_connect_info::<SocketAddr>(),
                )
                .map_err(|error| tracing::error!(?error, "Homeserver icann http server error")),
        );

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
                .serve(
                    router
                        .clone()
                        .into_make_service_with_connect_info::<SocketAddr>(),
                )
                .map_err(|error| tracing::error!(?error, "Homeserver pubky tls server error")),
        );

        Ok((http_handle, https_handle, http_socket, https_socket))
    }

    /// Get the URL of the icann http server.
    pub fn icann_http_url(&self) -> String {
        format!(
            "http://{}",
            self.icann_http_socket
        )
    }

    /// Get the URL of the pubky tls server with the Pubky DNS name.
    pub fn pubky_tls_dns_url(&self) -> String {
        format!("https://{}", self.context.keypair.public_key())
    }

    /// Get the URL of the pubky tls server with the Pubky IP address.
    pub fn pubky_tls_ip_url(&self) -> String {
        format!(
            "https://{}",
            self.pubky_tls_socket
        )
    }

    /// Shutdown the http and tls servers.
    pub fn shutdown(&self) {
        self.icann_http_handle.graceful_shutdown(Some(Duration::from_secs(5)));
        self.pubky_tls_handle.graceful_shutdown(Some(Duration::from_secs(5)));
    }
}

impl Drop for HomeserverCore {
    fn drop(&mut self) {
        self.shutdown();
    }
}
