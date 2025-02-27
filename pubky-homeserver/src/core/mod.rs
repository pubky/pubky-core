use std::path::PathBuf;

use anyhow::Result;
use axum::Router;
use pubky_common::auth::AuthVerifier;

pub mod database;
mod error;
mod extractors;
mod layers;
mod routes;

use crate::config::{
    DEFAULT_LIST_LIMIT, DEFAULT_MAP_SIZE, DEFAULT_MAX_LIST_LIMIT, DEFAULT_STORAGE_DIR,
};

use database::DB;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    pub(crate) db: DB,
    pub(crate) admin: AdminConfig,
}

#[derive(Debug, Clone)]
/// A side-effect-free Core of the [crate::Homeserver].
pub struct HomeserverCore {
    pub(crate) router: Router,
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
            db,
            admin,
        };

        let router = routes::create_app(state.clone());

        Ok(Self { router })
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SignupMode {
    Open,
    #[default]
    Closed,
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
            password: Some("test_admin_password".to_string()),
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
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            storage: storage(None)
                .expect("operating environment provides no directory for application data"),
            db_map_size: DEFAULT_MAP_SIZE,

            default_list_limit: DEFAULT_LIST_LIMIT,
            max_list_limit: DEFAULT_MAX_LIST_LIMIT,
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
}

pub fn storage(storage: Option<String>) -> Result<PathBuf> {
    let dir = if let Some(storage) = storage {
        PathBuf::from(storage)
    } else {
        let path = dirs_next::data_dir().ok_or_else(|| {
            anyhow::anyhow!("operating environment provides no directory for application data")
        })?;
        path.join(DEFAULT_STORAGE_DIR)
    };

    Ok(dir.join("homeserver"))
}
