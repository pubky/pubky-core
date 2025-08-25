use std::fmt::Debug;
use std::time::Duration;

use crate::errors::BuildError;

/// Transport-only client for Pubky. Reusable, stateless w.r.t. user identities.
pub const DEFAULT_RELAYS: &[&str] = &["https://pkarr.pubky.org/", "https://pkarr.pubky.app/"];
const DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"),);
const DEFAULT_MAX_RECORD_AGE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Default, Clone)]
pub struct PubkyClientBuilder {
    pkarr: pkarr::ClientBuilder,
    http_request_timeout: Option<Duration>,
    /// Maximum age before a user record should be republished.
    /// Defaults to 1 hour.
    max_record_age: Option<Duration>,
    /// Optional user-agent segment appended to the default UA for app-level telemetry.
    user_agent_extra: Option<String>,

    /// The hostname to use for testnet URL transformations (WASM only).
    #[cfg(target_arch = "wasm32")]
    testnet_host: Option<String>,
}

impl PubkyClientBuilder {
    #[cfg(not(target_arch = "wasm32"))]
    /// Creates a client connected to a local test network using `localhost`.
    /// To use a custom host, use `testnet_with_host`.
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

    /// Append an extra user-agent segment after the default `pubky.org@<version>`.
    /// Enables app-level telemetry
    /// Example: `.user_agent_extra("myapp/1.2.3")`
    pub fn user_agent_extra<S: Into<String>>(&mut self, extra: S) -> &mut Self {
        self.user_agent_extra = Some(extra.into());
        self
    }

    /// Set max age a record can have before it must be republished.
    /// Defaults to 1 hour if not overridden.
    pub fn max_record_age(&mut self, max_age: Duration) -> &mut Self {
        self.max_record_age = Some(max_age);
        self
    }

    /// Build [Client]
    pub fn build(&self) -> Result<PubkyClient, BuildError> {
        let pkarr = self.pkarr.build()?;

        // Compose user agent with optional extra part.
        let user_agent = match &self.user_agent_extra {
            Some(extra) if !extra.trim().is_empty() => {
                &format!("{DEFAULT_USER_AGENT} {}", extra.trim())
            }
            _ => DEFAULT_USER_AGENT,
        };

        #[cfg(not(target_arch = "wasm32"))]
        let mut http_builder = reqwest::ClientBuilder::from(pkarr.clone()).user_agent(user_agent);

        #[cfg(target_arch = "wasm32")]
        let http_builder = reqwest::Client::builder().user_agent(user_agent);

        #[cfg(not(target_arch = "wasm32"))]
        let mut icann_http_builder = reqwest::Client::builder().user_agent(user_agent);

        // TODO: change this after Reqwest publish a release with timeout in wasm
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(timeout) = self.http_request_timeout {
            http_builder = http_builder.timeout(timeout);

            icann_http_builder = icann_http_builder.timeout(timeout);
        }

        // Maximum age before a homeserver record should be republished.
        // Default is 1 hour. It's an arbitrary decision based only anecdotal evidence for DHT eviction.
        // See https://github.com/pubky/pkarr-churn/blob/main/results-node_decay.md for latest date of record churn
        let max_record_age = self.max_record_age.unwrap_or(DEFAULT_MAX_RECORD_AGE);

        Ok(PubkyClient {
            pkarr,
            http: http_builder.build()?,

            #[cfg(not(target_arch = "wasm32"))]
            icann_http: icann_http_builder.build()?,

            max_record_age,

            #[cfg(target_arch = "wasm32")]
            testnet_host: self.testnet_host.clone(),
        })
    }
}

/// Transport client for Pubky homeserver API and generic HTTP to Pubky and Icann URLs.
#[derive(Clone, Debug)]
pub struct PubkyClient {
    pub(crate) http: reqwest::Client,
    pub(crate) pkarr: pkarr::Client,

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) icann_http: reqwest::Client,

    /// The record age threshold before republishing.
    pub(crate) max_record_age: Duration,

    /// The hostname to use for testnet URL transformations (WASM only).
    #[cfg(target_arch = "wasm32")]
    pub(crate) testnet_host: Option<String>,
}

impl PubkyClient {
    /// Creates a client configured for public mainline DHT and pkarr relays.
    pub fn new() -> Result<PubkyClient, BuildError> {
        Self::builder().build()
    }

    /// Returns the current max record age threshold.
    pub fn max_record_age(&self) -> Duration {
        self.max_record_age
    }

    /// Returns a builder to edit settings before creating [Client].
    pub fn builder() -> PubkyClientBuilder {
        let mut builder = PubkyClientBuilder::default();
        builder.pkarr(|pkarr| pkarr.relays(DEFAULT_RELAYS).expect("infallible"));
        builder
    }

    /// Creates a client configured to use testnet DHT and Pkarr relays running on `localhost`.
    /// You need an instance of `pubky-testnet` running on `localhost`
    pub fn testnet() -> Result<PubkyClient, BuildError> {
        let mut builder = Self::builder();

        #[cfg(not(target_arch = "wasm32"))]
        builder.testnet();

        #[cfg(target_arch = "wasm32")]
        builder.testnet_host("localhost".to_string());

        builder.build()
    }

    // === Getters ===

    /// Returns a reference to the internal Pkarr Client.
    pub fn pkarr(&self) -> &pkarr::Client {
        &self.pkarr
    }
}

#[cfg(test)]
mod test {
    use reqwest::Method;

    use super::*;

    #[tokio::test]
    async fn test_fetch() {
        let client = PubkyClient::new().unwrap();
        let response = client
            .request(Method::GET, "https://example.com/")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
}
