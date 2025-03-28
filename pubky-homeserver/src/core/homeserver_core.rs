use std::time::Duration;

use super::periodic_backup::PeriodicBackup;
use super::key_republisher::HomeserverKeyRepublisher;
use crate::app_context::AppContext;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use crate::persistence::lmdb::LmDB;
use crate::SignupMode;
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
    #[allow(dead_code)] // Keep this alive. Republishing is stopped when the UserKeysRepublisher is dropped.
    pub(crate) user_keys_republisher: UserKeysRepublisher,
    #[allow(dead_code)] // Keep this alive. Republishing is stopped when the HomeserverKeyRepublisher is dropped.
    pub(crate) key_republisher: HomeserverKeyRepublisher,
    #[allow(dead_code)] // Keep this alive. Backup is stopped when the PeriodicBackup is dropped.
    pub(crate) periodic_backup: PeriodicBackup,
    pub(crate) router: Router,
    pub(crate) icann_http_handle: Option<Handle>,
    pub(crate) pubky_tls_handle: Option<Handle>,
}

impl HomeserverCore {
    /// Create a side-effect-free Homeserver core.
    pub async fn new(context: &AppContext) -> Result<Self> {
        let key_republisher = HomeserverKeyRepublisher::run(context).await?;
        let user_keys_republisher = UserKeysRepublisher::run_delayed(context, INITIAL_DELAY_BEFORE_REPUBLISH);
        let periodic_backup = PeriodicBackup::run(context);
        let router = Self::create_router(context);
        let (icann_http_handle, pubky_tls_handle) = Self::start_server_tasks(context).await?;

        Ok(Self {
            user_keys_republisher,
            key_republisher,
            periodic_backup,
            router,
            icann_http_handle: Some(icann_http_handle),
            pubky_tls_handle: Some(pubky_tls_handle),
        })
    }

    pub(crate) async fn listen(&mut self, context: &AppContext) -> Result<()> {
        let (icann_http_handle, pubky_tls_handle) = Self::start_server_tasks(context).await?;
        self.icann_http_handle = Some(icann_http_handle);
        self.pubky_tls_handle = Some(pubky_tls_handle);
        Ok(())
    }

    pub(crate) fn create_router(context: &AppContext) -> Router {
        let state = AppState {
            verifier: AuthVerifier::default(),
            db: context.db.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        };
        super::routes::create_app(state.clone())
    }

    /// Start the http and tls servers.
    async fn start_server_tasks(context: &AppContext) -> Result<(Handle, Handle)> {
        let router = Self::create_router(context);

        // Icann http server
        let http_listener = TcpListener::bind(context.config_toml.drive.icann_listen_socket)?;
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

        tracing::info!(
            "Homeserver HTTP listening on http://{}",
            context.config_toml.drive.icann_listen_socket
        );

        // Pubky tls server
        let https_listener = TcpListener::bind(context.config_toml.drive.pubky_listen_socket)?;

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
        tracing::info!(
            "Homeserver Pubky TLS listening on https://{} and http://{}",
            context.keypair.public_key(),
            context.config_toml.drive.icann_listen_socket
        );

        Ok((http_handle, https_handle))
    }

    /// Test version of [HomeserverCore::new], using an ephemeral small storage.
    pub async fn test() -> Result<Self> {
        // let mock_dir = DataDirMock::test();
        let context = AppContext::test();
        Self::new(&context).await
    }

    /// Stop the home server background tasks.
    #[allow(dead_code)]
    pub fn stop(self) {
        self.icann_http_handle.shutdown();
        self.pubky_tls_handle.shutdown();
    }
}