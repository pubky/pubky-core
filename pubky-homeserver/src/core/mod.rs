use anyhow::Result;
use axum::{extract::Request, response::Response, Router};
use pkarr::{Keypair, PublicKey};
use pubky_common::{
    auth::AuthVerifier, capabilities::Capability, crypto::random_bytes, session::Session,
    timestamp::Timestamp,
};
use tower::ServiceExt;
use tower_cookies::{cookie::SameSite, Cookie};

mod config;
mod database;
mod error;
mod extractors;
mod routes;

use database::{tables::users::User, DB};

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
    pub(crate) state: AppState,
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
            state,
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

    // TODO: move this logic to a common place.
    pub fn create_user(&mut self, public_key: &PublicKey) -> Result<Cookie> {
        let mut wtxn = self.state.db.env.write_txn()?;

        self.state.db.tables.users.put(
            &mut wtxn,
            public_key,
            &User {
                created_at: Timestamp::now().as_u64(),
            },
        )?;

        let session_secret = base32::encode(base32::Alphabet::Crockford, &random_bytes::<16>());

        let session = Session::new(public_key, &[Capability::root()], None).serialize();

        self.state
            .db
            .tables
            .sessions
            .put(&mut wtxn, &session_secret, &session)?;

        wtxn.commit()?;

        let mut cookie = if true {
            Cookie::new("session_id", session_secret)
        } else {
            Cookie::new(public_key.to_string(), session_secret)
        };

        cookie.set_path("/");

        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
        cookie.set_http_only(true);

        Ok(cookie)
    }

    pub async fn call(&self, request: Request) -> Result<Response> {
        Ok(self.router.clone().oneshot(request).await?)
    }
}
