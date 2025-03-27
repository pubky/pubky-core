use std::time::Duration;

use crate::app_context::AppContext;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use crate::persistence::lmdb::LmDB;
use crate::SignupMode;
use anyhow::Result;
use axum::Router;
use axum_server::{tls_rustls::{RustlsAcceptor, RustlsConfig}, Handle};
use pubky_common::auth::AuthVerifier;
use tokio::time::sleep;
use futures_util::TryFutureExt;
use super::backup::backup_lmdb_periodically;
use super::key_republisher::HomeserverKeyRepublisher;
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

#[derive(Debug, Clone)]
/// A side-effect-free Core of the [crate::Homeserver].
pub struct HomeserverCore {
    pub(crate) user_keys_republisher: UserKeysRepublisher,
    pub(crate) key_republisher: HomeserverKeyRepublisher,
    pub(crate) icann_http_handle: Handle,
    pub(crate) pubky_tls_handle: Handle,
    #[cfg(any(test, feature = "testing"))]
    pub(crate) router: Router,
}

impl HomeserverCore {
    /// Create a side-effect-free Homeserver core.
    pub async fn new(context: &AppContext) -> Result<Self> {
        // TODO: Tasks get started here. But if new() fails due to an error, the tasks should be stopped.
        let key_republisher = Self::start_key_republisher(context).await?;
        Self::start_backup_task(context).await;
        let user_keys_republisher = Self::start_user_keys_republisher(context).await?;

        let (icann_http_handle, pubky_tls_handle, router) = Self::start_server_tasks(context).await?;

        Ok(Self {
            user_keys_republisher,
            key_republisher,
            icann_http_handle,
            pubky_tls_handle,
            #[cfg(any(test, feature = "testing"))]
            router,
        })
    }

    /// Background task to republish the homeserver's pkarr packet to the DHT.
    async fn start_key_republisher(context: &AppContext) -> Result<HomeserverKeyRepublisher> {
        let key_republisher = HomeserverKeyRepublisher::new(context)?;
        key_republisher.start_periodic_republish().await?;
        Ok(key_republisher)
    }

    /// Spawn the backup process. This task will run forever.
    async fn start_backup_task(context: &AppContext) {
        let backup_interval = context.config_toml.general.lmdb_backup_interval_s;
        if backup_interval > 0 {
            let backup_path = context.data_dir.path().join("backup");
            tokio::spawn(backup_lmdb_periodically(
                context.db.clone(),
                backup_path,
                Duration::from_secs(backup_interval),
            ));
        }
    }

    /// Background task to republish the user keys to the DHT.
    async fn start_user_keys_republisher(context: &AppContext) -> Result<UserKeysRepublisher> {
        // Background task to republish the user keys to the DHT.
        let user_keys_republisher_interval =
            context.config_toml.pkdns.user_keys_republisher_interval;
        let user_keys_republisher = UserKeysRepublisher::new(
            context.db.clone(),
            Duration::from_secs(user_keys_republisher_interval),
        );
        if user_keys_republisher_interval > 0 {
            // Delayed start of the republisher to give time for the homeserver to start.
            let user_keys_republisher_clone = user_keys_republisher.clone();
            tokio::spawn(async move {
                sleep(INITIAL_DELAY_BEFORE_REPUBLISH).await;
                user_keys_republisher_clone.run().await;
            });
        }
        Ok(user_keys_republisher)
    }

    /// Start the http and tls servers.
    async fn start_server_tasks(context: &AppContext) -> Result<(Handle, Handle, Router)> {
        let state = AppState {
            verifier: AuthVerifier::default(),
            db: context.db.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        };
        let router = super::routes::create_app(state.clone());

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
                .map_err(|error| tracing::error!(?error, "Homeserver icann http server error"))
        );

        tracing::info!(
            "Homeserver HTTP listening on http://{}",
            context.config_toml.drive.icann_listen_socket
        );

        // Pubky tls server
        let https_listener =
            TcpListener::bind(context.config_toml.drive.pubky_listen_socket)?;

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
                .map_err(|error| tracing::error!(?error, "Homeserver pubky tls server error"))    
        );
        tracing::info!("Homeserver Pubky TLS listening on https://{} and http://{}", context.keypair.public_key(), context.config_toml.drive.icann_listen_socket);

        Ok((
            http_handle,
            https_handle,
            router,
        ))
    }

    /// Stop the home server background tasks.
    #[allow(dead_code)]
    pub async fn stop(&mut self) {
        self.key_republisher.stop_periodic_republish().await;
        self.user_keys_republisher.stop().await;
        self.icann_http_handle.shutdown();
        self.pubky_tls_handle.shutdown();
    }
}

#[cfg(test)]
mod tests {

    use anyhow::Result;
    use axum::{
        body::Body,
        extract::Request,
        http::{header, Method},
        response::Response,
    };
    use pkarr::Keypair;
    use pubky_common::{auth::AuthToken, capabilities::Capability};
    use tower::ServiceExt;

    use super::*;

    impl HomeserverCore {
        /// Test version of [HomeserverCore::new], using an ephemeral small storage.
        pub async fn test() -> Result<Self> {
            let context = AppContext::test();
            Self::new(&context).await
        }

        // === Public Methods ===

        pub async fn create_root_user(&mut self, keypair: &Keypair) -> Result<String> {
            let auth_token = AuthToken::sign(keypair, vec![Capability::root()]);

            let response = self
                .call(
                    Request::builder()
                        .uri("/signup")
                        .header("host", keypair.public_key().to_string())
                        .method(Method::POST)
                        .body(Body::from(auth_token.serialize()))
                        .unwrap(),
                )
                .await?;

            let header_value = response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|h| h.to_str().ok())
                .expect("should return a set-cookie header")
                .to_string();

            Ok(header_value)
        }

        pub async fn call(&self, request: Request) -> Result<Response> {
            Ok(self.router.clone().oneshot(request).await?)
        }
    }
}
