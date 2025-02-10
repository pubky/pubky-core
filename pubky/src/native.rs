pub mod internal {
    #[cfg(not(wasm_browser))]
    pub mod cookies;
    pub mod pkarr;
}
pub mod api {
    pub mod auth;
    #[cfg(not(wasm_browser))]
    pub mod http;
    pub mod public;
}

use std::fmt::Debug;

#[cfg(not(wasm_browser))]
use std::sync::Arc;
use std::time::Duration;

#[cfg(not(wasm_browser))]
use mainline::Testnet;

static DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"),);

#[macro_export]
macro_rules! handle_http_error {
    ($res:expr) => {
        if let Err(status) = $res.error_for_status_ref() {
            return match $res.text().await {
                Ok(text) => Err(anyhow::anyhow!("{status}. Error message: {text}")),
                _ => Err(anyhow::anyhow!("{status}")),
            };
        }
    };
}

#[derive(Debug, Default, Clone)]
pub struct ClientBuilder {
    pkarr: pkarr::ClientBuilder,
    http_request_timeout: Option<Duration>,
}

impl ClientBuilder {
    #[cfg(not(wasm_browser))]
    /// Sets the following:
    /// - Pkarr client's DHT bootstrap nodes = `testnet` bootstrap nodes.
    /// - Pkarr client's resolvers           = `testnet` bootstrap nodes.
    /// - Pkarr client's DHT request timeout  = 500 milliseconds. (unless in CI, then it is left as default 2000)
    pub fn testnet(&mut self, testnet: &Testnet) -> &mut Self {
        let bootstrap = testnet.bootstrap.clone();

        self.pkarr.no_default_network().bootstrap(&bootstrap);

        if std::env::var("CI").is_err() {
            self.pkarr.request_timeout(Duration::from_millis(500));
        }

        self
    }

    /// Create a [mainline::DhtBuilder] if `None`, and allows mutating it with a callback function.
    pub fn pkarr<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut pkarr::ClientBuilder) -> &mut pkarr::ClientBuilder,
    {
        f(&mut self.pkarr);

        self
    }

    /// Set HTTP requests timeout.
    pub fn request_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.http_request_timeout = Some(timeout);

        self
    }

    /// Build [Client]
    pub fn build(&self) -> Result<Client, BuildError> {
        let pkarr = self.pkarr.build()?;

        #[cfg(not(wasm_browser))]
        let cookie_store = Arc::new(internal::cookies::CookieJar::default());

        // TODO: allow custom user agent, but force a Pubky user agent information
        let user_agent = DEFAULT_USER_AGENT;

        #[cfg(not(wasm_browser))]
        let mut http_builder = reqwest::ClientBuilder::from(pkarr.clone())
            // TODO: use persistent cookie jar
            .cookie_provider(cookie_store.clone())
            .user_agent(user_agent);

        #[cfg(wasm_browser)]
        let mut http_builder = reqwest::Client::builder().user_agent(user_agent);

        #[cfg(not(wasm_browser))]
        let mut icann_http_builder = reqwest::Client::builder()
            // TODO: use persistent cookie jar
            .cookie_provider(cookie_store.clone())
            .user_agent(user_agent);

        // TODO: change this after Reqwest publish a release with timeout in wasm
        #[cfg(not(wasm_browser))]
        if let Some(timeout) = self.http_request_timeout {
            http_builder = http_builder.timeout(timeout);

            icann_http_builder = icann_http_builder.timeout(timeout);
        }

        Ok(Client {
            pkarr,
            http: http_builder.build().expect("config expected to not error"),

            #[cfg(not(wasm_browser))]
            icann_http: icann_http_builder
                .build()
                .expect("config expected to not error"),
            #[cfg(not(wasm_browser))]
            cookie_store,

            #[cfg(wasm_browser)]
            testnet: false,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    /// Error building Pkarr client.
    PkarrBuildError(#[from] pkarr::errors::BuildError),
}

/// A client for Pubky homeserver API, as well as generic HTTP requests to Pubky urls.
#[derive(Clone, Debug)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    pub(crate) pkarr: pkarr::Client,

    #[cfg(not(wasm_browser))]
    pub(crate) cookie_store: std::sync::Arc<internal::cookies::CookieJar>,
    #[cfg(not(wasm_browser))]
    pub(crate) icann_http: reqwest::Client,

    #[cfg(wasm_browser)]
    pub(crate) testnet: bool,
}

impl Client {
    /// Returns a builder to edit settings before creating [Client].
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    #[cfg(not(wasm_browser))]
    /// Create a client connected to the local network
    /// with the bootstrapping node: `localhost:6881`
    pub fn testnet() -> Result<Self, BuildError> {
        Self::builder()
            .testnet(&Testnet {
                bootstrap: vec!["localhost:6881".to_string()],
                nodes: vec![],
            })
            .build()
    }

    #[cfg(test)]
    #[cfg(not(wasm_browser))]
    /// Alias to `pubky::Client::builder().testnet(testnet).build().unwrap()`
    pub(crate) fn test(testnet: &Testnet) -> Client {
        Client::builder().testnet(testnet).build().unwrap()
    }
}
