use std::{path::PathBuf, time::Duration};

use crate::core::database::DB;
use crate::core::user_keys_republisher::UserKeysRepublisher;
use anyhow::Result;
use axum::Router;
use pubky_common::auth::AuthVerifier;
use tokio::time::sleep;

pub const DEFAULT_REPUBLISHER_INTERVAL: u64 = 4 * 60 * 60; // 4 hours in seconds

pub const DEFAULT_STORAGE_DIR: &str = "pubky";
pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    pub(crate) db: DB,
    pub(crate) admin: AdminConfig,
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
    pub unsafe fn new(config: CoreConfig, admin: AdminConfig) -> Result<Self> {
        let db = unsafe { DB::open(config.clone())? };

        let state = AppState {
            verifier: AuthVerifier::default(),
            db: db.clone(),
            admin,
        };

        let router = super::routes::create_app(state.clone());

        let user_keys_republisher = UserKeysRepublisher::new(
            db.clone(),
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SignupMode {
    Open,
    #[default]
    TokenRequired,
}

impl TryFrom<String> for SignupMode {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(match value.as_str() {
            "open" => Self::Open,
            "token_required" => Self::TokenRequired,
            _ => return Err(anyhow::anyhow!("Invalid signup mode: {}", value)),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdminConfig {
    /// The password used to authorize admin endpoints.
    pub password: Option<String>,
    /// Determines whether new signups require a valid token.
    pub signup_mode: SignupMode,
}

impl AdminConfig {
    pub fn test() -> Self {
        AdminConfig {
            password: Some("admin".to_string()),
            signup_mode: SignupMode::Open,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Database configurations
pub struct CoreConfig {
    /// Path to the storage directory.
    ///
    /// Defaults to a directory in the OS data directory
    pub storage: PathBuf,
    pub db_map_size: usize,

    /// The default limit of a list api if no `limit` query parameter is provided.
    ///
    /// Defaults to `100`
    pub default_list_limit: u16,
    /// The maximum limit of a list api, even if a `limit` query parameter is provided.
    ///
    /// Defaults to `1000`
    pub max_list_limit: u16,

    /// The interval at which the user keys republisher runs. None is disabled.
    ///
    /// Defaults to `60*60*4` (4 hours)
    pub user_keys_republisher_interval: Option<Duration>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            storage: storage(None)
                .expect("operating environment provides no directory for application data"),
            db_map_size: DEFAULT_MAP_SIZE,

            default_list_limit: DEFAULT_LIST_LIMIT,
            max_list_limit: DEFAULT_MAX_LIST_LIMIT,

            user_keys_republisher_interval: Some(Duration::from_secs(60 * 60 * 4)),
        }
    }
}

impl CoreConfig {
    pub fn test() -> Self {
        let storage = std::env::temp_dir()
            .join(pubky_common::timestamp::Timestamp::now().to_string())
            .join(DEFAULT_STORAGE_DIR);

        Self {
            storage,
            db_map_size: 10485760,

            ..Default::default()
        }
    }

    pub fn is_user_keys_republisher_enabled(&self) -> bool {
        self.user_keys_republisher_interval.is_some()
    }
}

pub fn storage(storage: Option<String>) -> anyhow::Result<PathBuf> {
    if let Some(storage) = storage {
        Ok(PathBuf::from(storage))
    } else {
        dirs::home_dir()
        .map(|dir| dir.join(".pubky/data/lmdb"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "operating environment provides no directory for application data"
            )
        })
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
            unsafe { HomeserverCore::new(CoreConfig::test(), AdminConfig::test()) }
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
