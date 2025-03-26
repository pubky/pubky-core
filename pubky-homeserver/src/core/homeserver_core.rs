use std::net::{Ipv4Addr, SocketAddr};
use std::{net::IpAddr, time::Duration};

use crate::context::AppContext;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use crate::SignupMode;
use crate::{persistence::lmdb::LmDB, Domain};
use anyhow::Result;
use axum::Router;
use pkarr::Keypair;
use pubky_common::auth::AuthVerifier;
use tokio::time::sleep;
use crate::constants::{default_keypair, DEFAULT_ICANN_HTTP_LISTEN_SOCKET, DEFAULT_PUBKY_TLS_LISTEN_SOCKET};

use super::backup::backup_lmdb_periodically;
use super::key_republisher::HomeserverKeyRepublisher;

pub const DEFAULT_REPUBLISHER_INTERVAL: u64 = 4 * 60 * 60; // 4 hours in seconds



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
    pub(crate) router: Router,
    pub(crate) user_keys_republisher: UserKeysRepublisher,
    pub(crate) key_republisher: HomeserverKeyRepublisher,
}

impl HomeserverCore {
    /// Create a side-effect-free Homeserver core.
    ///
    /// # Safety
    /// HomeserverCore uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub async fn new(context: &AppContext) -> Result<Self> {
        let state = AppState {
            verifier: AuthVerifier::default(),
            db: context.db.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        };

        // Spawn the backup process. This task will run forever.
        let backup_interval = context.config_toml.general.lmdb_backup_interval_s;
        if backup_interval > 0 {
            let backup_path = context.data_dir.path().join("backup");
            tokio::spawn(backup_lmdb_periodically(
                context.db.clone(),
                backup_path,
                Duration::from_secs(backup_interval),
            ));
        }

        let router = super::routes::create_app(state.clone());

        // Background task to republish the homeserver's pkarr packet to the DHT.
        let key_republisher = HomeserverKeyRepublisher::new(context)?;
        key_republisher.start_periodic_republish().await?;

        // Background task to republish the user keys to the DHT.
        let user_keys_republisher_interval = context.config_toml.pkdns.user_keys_republisher_interval;
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

        Ok(Self {
            router,
            user_keys_republisher,
            key_republisher,
        })
    }

    /// Stop the home server background tasks.
    #[allow(dead_code)]
    pub async fn stop(&mut self) {
        self.key_republisher.stop_periodic_republish().await;
        self.user_keys_republisher.stop().await;
    }
}

#[derive(Debug, Clone)]
/// Homeserver core configuration
pub struct CoreConfig {
    /// The keypair of the homeserver
    pub(crate) keypair: Keypair,

    /// The database to use
    pub(crate) db: LmDB,

    /// The interval at which the user keys republisher runs. None is disabled.
    ///
    /// Defaults to `60*60*4` (4 hours)
    pub (crate) user_keys_republisher_interval: Option<Duration>,

    /// The interval at which the LMDB backup is performed. None means disabled.
    pub (crate) lmdb_backup_interval: Option<Duration>,

    /// The mode of the signup.
    pub(crate) signup_mode: SignupMode,

    /// The builder for the pkarr client.
    pub(crate) pkarr_builder: pkarr::ClientBuilder,

    /// The public ip address of the homeserver.
    /// Default: 127.0.0.1
    pub(crate) public_ip: IpAddr,

    /// The port of the pubky tls server.
    /// If not set, pubky_tls_listen.port() is used.
    pub(crate) public_pubky_tls_port: Option<u16>,

    /// The port of the icann http server.
    /// If not set, icann_http_listen.port() is used.
    pub(crate) public_icann_http_port: Option<u16>,

    /// The domain of the homeserver.
    /// Default: "localhost"
    pub(crate) domain: Domain,

    /// The socket address of the pubky tls server.
    pub(crate) pubky_tls_listen: SocketAddr,

    /// The socket address of the icann http server.
    pub(crate) icann_http_listen: SocketAddr,
}

impl CoreConfig {
    pub fn new(
        db: LmDB
    ) -> Self {
        Self {
            keypair: default_keypair(),
            db,
            user_keys_republisher_interval: None,
            signup_mode: SignupMode::TokenRequired,
            pkarr_builder: pkarr::ClientBuilder::default(),
            public_ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            public_pubky_tls_port: None,
            public_icann_http_port: None,
            domain: Domain::default(),
            pubky_tls_listen: DEFAULT_PUBKY_TLS_LISTEN_SOCKET,
            icann_http_listen: DEFAULT_ICANN_HTTP_LISTEN_SOCKET,
            lmdb_backup_interval: None,
        }
    }

    pub fn keypair(&mut self, keypair: Keypair) -> &mut Self {
        self.keypair = keypair;
        self
    }

    pub fn signup_mode(&mut self, signup_mode: SignupMode) -> &mut Self {
        self.signup_mode = signup_mode;
        self
    }

    pub fn user_keys_republisher_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.user_keys_republisher_interval = interval;
        self
    }

    pub fn is_user_keys_republisher_enabled(&self) -> bool {
        self.user_keys_republisher_interval.is_some()
    }

    pub fn pkarr_builder(&mut self, pkarr_builder: pkarr::ClientBuilder) -> &mut Self {
        self.pkarr_builder = pkarr_builder;
        self
    }

    /// Set the public ip address of the homeserver so others can find it.
    /// Default: 127.0.0.1
    pub fn public_ip(&mut self, public_ip: IpAddr) -> &mut Self {
        self.public_ip = public_ip;
        self
    }

    /// Set the port of the pubky tls server so others can find it.
    /// If not set, pubky_tls_listen.port() is used.
    pub fn public_pubky_tls_port(&mut self, public_pubky_tls_port: u16) -> &mut Self {
        self.public_pubky_tls_port = Some(public_pubky_tls_port);
        self
    }

    /// Set the port of the icann http server so others can find it.
    /// If not set, icann_http_listen.port() is used.
    pub fn public_icann_http_port(&mut self, public_icann_http_port: u16) -> &mut Self {
        self.public_icann_http_port = Some(public_icann_http_port);
        self
    }

    /// Set the domain of the homeserver so others can find it.
    /// Default: "localhost"
    pub fn domain(&mut self, domain: Domain) -> &mut Self {
        self.domain = domain;
        self
    }

    /// Set the socket listen address of the pubky tls server.
    pub fn pubky_tls_listen(&mut self, pubky_tls_listen: SocketAddr) -> &mut Self {
        self.pubky_tls_listen = pubky_tls_listen;
        self
    }

    /// Set the socket listen address of the icann http server.
    pub fn icann_http_listen(&mut self, icann_http_listen: SocketAddr) -> &mut Self {
        self.icann_http_listen = icann_http_listen;
        self
    }

    /// Derive the public ports from the listen and override ports.
    pub fn get_public_pubky_tls_port(&self) -> u16 {
        self.public_pubky_tls_port
            .unwrap_or(self.pubky_tls_listen.port())
    }

    /// Derive the public ports from the listen and override ports.
    pub fn get_public_icann_http_port(&self) -> u16 {
        self.public_icann_http_port
            .unwrap_or(self.icann_http_listen.port())
    }

    #[cfg(test)]
    pub fn test() -> Self {
        use std::net::Ipv4Addr;

        Self {
            keypair: Keypair::random(),
            db: LmDB::test(),
            user_keys_republisher_interval: None,
            signup_mode: SignupMode::TokenRequired,
            pkarr_builder: pkarr::ClientBuilder::default(),
            public_ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            public_pubky_tls_port: None,
            public_icann_http_port: None,
            domain: Domain::default(),
            pubky_tls_listen: DEFAULT_PUBKY_TLS_LISTEN_SOCKET,
            icann_http_listen: DEFAULT_ICANN_HTTP_LISTEN_SOCKET,
            lmdb_backup_interval: None,
        }
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
