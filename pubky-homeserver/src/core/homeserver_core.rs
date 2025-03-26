use std::time::Duration;

use crate::persistence::lmdb::LmDB;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use crate::SignupMode;
use anyhow::Result;
use axum::Router;
use pkarr::Keypair;
use pubky_common::auth::AuthVerifier;
use tokio::time::sleep;

use super::key_republisher::{HomeserverKeyRepublisher, HomeserverKeyRepublisherConfig};

pub const DEFAULT_REPUBLISHER_INTERVAL: u64 = 4 * 60 * 60; // 4 hours in seconds

pub const DEFAULT_STORAGE_DIR: &str = "pubky";
pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

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
}

impl HomeserverCore {
    /// Create a side-effect-free Homeserver core.
    ///
    /// # Safety
    /// HomeserverCore uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub async unsafe fn new(config: CoreConfig) -> Result<Self> {

        let state = AppState {
            verifier: AuthVerifier::default(),
            db: config.db.clone(),
            signup_mode: config.signup_mode.clone(),
        };

        let router = super::routes::create_app(state.clone());

        let pkarr_client = config.pkarr_builder.build()?;
        let republisher_config = HomeserverKeyRepublisherConfig::new(config.keypair.clone(), public_ip, pubky_https_port, icann_http_port, pkarr_client);
        let dht_republisher = HomeserverKeyRepublisher::new(republisher_config)?;
        dht_republisher.start_periodic_republish().await?;

        let user_keys_republisher = UserKeysRepublisher::new(
            config.db.clone(),
            config
                .user_keys_republisher_interval
                .unwrap_or(Duration::from_secs(DEFAULT_REPUBLISHER_INTERVAL)),
        );

        let user_keys_republisher_clone = user_keys_republisher.clone();
        if config.is_user_keys_republisher_enabled() {
            // Delayed start of the republisher to give time for the homeserver to start.
            tokio::spawn(async move {
                sleep(INITIAL_DELAY_BEFORE_REPUBLISH).await;
                user_keys_republisher_clone.run().await;
            });
        }
        Ok(Self {
            router,
            user_keys_republisher,
        })
    }

    /// Stop the home server background tasks.
    #[allow(dead_code)]
    pub async fn stop(&mut self) {
        self.user_keys_republisher.stop().await;
    }
}



#[derive(Debug, Clone)]
/// Database configurations
pub struct CoreConfig {
    pub(crate) keypair: Keypair,

    /// The database to use
    pub(crate) db: LmDB,

    /// The interval at which the user keys republisher runs. None is disabled.
    ///
    /// Defaults to `60*60*4` (4 hours)
    pub(crate) user_keys_republisher_interval: Option<Duration>,

    pub(crate) signup_mode: SignupMode,

    pub(crate) pkarr_builder: pkarr::ClientBuilder,
}


impl CoreConfig {
    pub fn new(keypair: Keypair, db: LmDB) -> Self {
        Self {
            keypair,
            db,
            user_keys_republisher_interval: None,
            signup_mode: SignupMode::Open,
            pkarr_builder: pkarr::ClientBuilder::default(),
        }
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

    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            keypair: Keypair::random(),
            db: LmDB::test(),
            user_keys_republisher_interval: None,
            signup_mode: SignupMode::Open,
            pkarr_builder: pkarr::ClientBuilder::default(),
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
        pub fn test() -> Result<Self> {
            unsafe { Self::new(CoreConfig::test()) }
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
