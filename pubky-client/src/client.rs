use std::fmt::Debug;

#[cfg(not(target_arch = "wasm32"))]
use super::internal::cookies::CookieJar;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
use std::time::Duration;

static DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"),);
static DEFAULT_RELAYS: &[&str] = &["https://pkarr.pubky.org/", "https://pkarr.pubky.app/"];

#[derive(Debug, Default, Clone)]
pub struct ClientBuilder {
    pkarr: pkarr::ClientBuilder,
    http_request_timeout: Option<Duration>,
    /// Maximum age before a user record should be republished.
    /// Defaults to 1 hour.
    max_record_age: Option<Duration>,
    /// The hostname to use for testnet URL transformations (WASM only).
    #[cfg(target_arch = "wasm32")]
    testnet_host: Option<String>,
}

impl ClientBuilder {
    #[cfg(not(target_arch = "wasm32"))]
    /// Creates a client connected to a local test network using `localhost`.
    /// To use a custom host, see `testnet_with_host`.
    pub fn testnet(&mut self) -> &mut Self {
        self.testnet_with_host("localhost")
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Creates a client connected to a local test network with a custom homeserver
    /// host other than `localhost`.
    ///
    /// Configures:
    /// 1. local DHT with bootstrapping nodes: `&["<host>:6881"]`
    /// 2. Pkarr Relay: `http://<host>:<PKARR_RELAY_PORT>`
    pub fn testnet_with_host(&mut self, host: &str) -> &mut Self {
        self.pkarr
            .bootstrap(&[format!(
                "{}:{}",
                host,
                pubky_common::constants::testnet_ports::BOOTSTRAP
            )])
            .relays(&[format!(
                "http://{}:{}",
                host,
                pubky_common::constants::testnet_ports::PKARR_RELAY
            )])
            .expect("relays urls infallible");

        self
    }

    /// Sets the testnet host. This is only used for WASM builds.
    pub fn testnet_host(&mut self, host: String) -> &mut Self {
        // The field itself is still conditional, so the logic is gated.
        #[cfg(target_arch = "wasm32")]
        {
            self.testnet_host = Some(host);
        }
        // This avoids an "unused parameter" warning on non-WASM builds.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = host;

        self
    }

    /// Allows mutating the internal [pkarr::ClientBuilder] with a callback function.
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

    /// Set max age a record can have before it must be republished.
    /// Defaults to 1 hour if not overridden.
    pub fn max_record_age(&mut self, max_age: Duration) -> &mut Self {
        self.max_record_age = Some(max_age);
        self
    }

    /// Build [Client]
    pub fn build(&self) -> Result<Client, BuildError> {
        let pkarr = self.pkarr.build()?;

        #[cfg(not(target_arch = "wasm32"))]
        let cookie_store = Arc::new(CookieJar::default());

        // TODO: allow custom user agent, but force a Pubky user agent information
        let user_agent = DEFAULT_USER_AGENT;

        #[cfg(not(target_arch = "wasm32"))]
        let mut http_builder = reqwest::ClientBuilder::from(pkarr.clone())
            // TODO: use persistent cookie jar
            .cookie_provider(cookie_store.clone())
            .user_agent(user_agent);

        #[cfg(target_arch = "wasm32")]
        let http_builder = reqwest::Client::builder().user_agent(user_agent);

        #[cfg(not(target_arch = "wasm32"))]
        let mut icann_http_builder = reqwest::Client::builder()
            // TODO: use persistent cookie jar
            .cookie_provider(cookie_store.clone())
            .user_agent(user_agent);

        // TODO: change this after Reqwest publish a release with timeout in wasm
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(timeout) = self.http_request_timeout {
            http_builder = http_builder.timeout(timeout);

            icann_http_builder = icann_http_builder.timeout(timeout);
        }

        // Maximum age before a homeserver record should be republished.
        // Default is 1 hour. It's an arbitrary decision based only anecdotal evidence for DHT eviction.
        // See https://github.com/pubky/pkarr-churn/blob/main/results-node_decay.md for latest date of record churn
        let max_record_age = self.max_record_age.unwrap_or(Duration::from_secs(60 * 60));

        Ok(Client {
            pkarr,
            http: http_builder.build().expect("config expected to not error"),

            #[cfg(not(target_arch = "wasm32"))]
            icann_http: icann_http_builder
                .build()
                .expect("config expected to not error"),
            #[cfg(not(target_arch = "wasm32"))]
            cookie_store,

            max_record_age,

            #[cfg(target_arch = "wasm32")]
            testnet_host: self.testnet_host.clone(),
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cookie_store: std::sync::Arc<CookieJar>,

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) icann_http: reqwest::Client,

    /// The record age threshold before republishing.
    pub(crate) max_record_age: Duration,

    /// The hostname to use for testnet URL transformations (WASM only).
    #[cfg(target_arch = "wasm32")]
    pub(crate) testnet_host: Option<String>,
}

impl Client {
    /// Returns a builder to edit settings before creating [Client].
    pub fn builder() -> ClientBuilder {
        let mut builder = ClientBuilder::default();
        builder.pkarr(|pkarr| pkarr.relays(DEFAULT_RELAYS).expect("infallible"));
        builder
    }

    // === Getters ===

    /// Returns a reference to the internal Pkarr Client.
    pub fn pkarr(&self) -> &pkarr::Client {
        &self.pkarr
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_fetch() {
        let client = Client::builder().build().unwrap();
        let response = client.get("https://google.com/").send().await.unwrap();
        assert_eq!(response.status(), 200);
    }
}
