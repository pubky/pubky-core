use anyhow::Result;
use axum::{
    body::Body,
    extract::Request,
    http::{header, Method},
    response::Response,
    Router,
};
use pkarr::{Keypair, PublicKey};
use pubky_common::{
    auth::{AuthToken, AuthVerifier},
    capabilities::Capability,
};
use tower::ServiceExt;

mod config;
mod database;
mod error;
mod extractors;
mod layers;
mod routes;

use database::DB;

pub use config::Config;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    pub(crate) db: DB,
}

#[derive(Debug, Clone)]
/// A side-effect-free Core of the [Homeserver].
pub struct HomeserverCore {
    config: Config,
    pub(crate) router: Router,
}

impl HomeserverCore {
    /// Create a side-effect-free Homeserver core.
    ///
    /// # Safety
    /// HomeserverCore uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub unsafe fn new(config: &Config) -> Result<Self> {
        let db = unsafe { DB::open(config.clone())? };

        let state = AppState {
            verifier: AuthVerifier::default(),
            db,
        };

        let router = routes::create_app(state.clone());

        Ok(Self {
            router,
            config: config.clone(),
        })
    }

    #[cfg(test)]
    /// Test version of [HomeserverCore::new], using a temporary storage.
    pub fn test() -> Result<Self> {
        let testnet = pkarr::mainline::Testnet::new(0).expect("ignore");

        unsafe { HomeserverCore::new(&Config::test(&testnet)) }
    }

    // === Getters ===

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn keypair(&self) -> &Keypair {
        &self.config.keypair
    }

    pub fn public_key(&self) -> PublicKey {
        self.config.keypair.public_key()
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
