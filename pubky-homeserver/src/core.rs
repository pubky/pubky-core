use std::net::{SocketAddr, TcpListener};

use anyhow::Result;
use axum::{extract::Request, response::Response, Router};
use pkarr::PublicKey;
use pubky_common::{
    auth::AuthVerifier, capabilities::Capability, crypto::random_bytes, session::Session,
    timestamp::Timestamp,
};
use tower::ServiceExt;
use tower_cookies::{cookie::SameSite, Cookie};

use crate::{
    config::Config,
    database::{tables::users::User, DB},
};

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    pub(crate) db: DB,
    pub(crate) pkarr_client: pkarr::Client,
    pub(crate) config: Config,
    pub(crate) port: u16,
}

#[derive(Debug)]
/// An I/O-less Core of the [Homeserver].
pub struct HomeserverCore {
    pub(crate) state: AppState,
    pub(crate) router: Router,
}

impl HomeserverCore {
    pub fn new(config: &Config) -> Result<Self> {
        tracing::debug!(?config);

        let db = DB::open(config.clone())?;

        let mut dht_settings = pkarr::mainline::Settings::default();

        if let Some(bootstrap) = config.bootstrap() {
            dht_settings = dht_settings.bootstrap(&bootstrap);
        }
        if let Some(request_timeout) = config.dht_request_timeout() {
            dht_settings = dht_settings.request_timeout(request_timeout);
        }

        let pkarr_client = pkarr::Client::builder()
            .dht_settings(dht_settings)
            .build()?;

        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.port())))?;

        let port = listener.local_addr()?.port();

        let state = AppState {
            verifier: AuthVerifier::default(),
            db,
            pkarr_client: pkarr_client.clone(),
            config: config.clone(),
            port,
        };

        let router = crate::routes::create_app(state.clone());

        Ok(Self { state, router })
    }

    #[cfg(test)]
    /// Test version of [HomeserverCore::new], using a temporary storage.
    pub fn test() -> Result<Self> {
        let testnet = pkarr::mainline::Testnet::new(0).expect("ignore");

        HomeserverCore::new(&Config::test(&testnet))
    }

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
