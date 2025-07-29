use super::http_client::HttpClient;
use std::{fmt::Debug, time::Duration};

// Static constants remain unchanged.
pub static DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"));
pub static DEFAULT_RELAYS: &[&str] = &["https://pkarr.pubky.org/", "https://pkarr.pubky.app/"];

/// Holds the platform-agnostic configuration for a `Client`.
///
/// This struct is used to configure and build the necessary components (like `pkarr::Client`)
/// before they are combined with a platform-specific `HttpClient` to create a full `Client`.
#[derive(Debug, Default, Clone)]
pub struct ClientConfig {
    pkarr: pkarr::ClientBuilder,
    pub(crate) max_record_age: Option<Duration>,
}

impl ClientConfig {
    /// Creates a new configuration with default settings, including default Pkarr relays.
    pub fn new() -> Self {
        let mut config = Self::default();
        config.pkarr(|pkarr| {
            pkarr
                .relays(DEFAULT_RELAYS)
                .expect("Default relays are valid")
        });
        config
    }
    /// Allows mutating the internal [pkarr::ClientBuilder] with a callback function.
    pub fn pkarr<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut pkarr::ClientBuilder) -> &mut pkarr::ClientBuilder,
    {
        f(&mut self.pkarr);
        self
    }

    /// Set max age a record can have before it must be republished.
    /// Defaults to 1 hour if not overridden.
    pub fn max_record_age(&mut self, max_age: Duration) -> &mut Self {
        self.max_record_age = Some(max_age);
        self
    }

    /// Builds the `pkarr::Client` from the specified configuration.
    pub fn build_pkarr_client(&self) -> Result<pkarr::Client, BuildError> {
        self.pkarr.build().map_err(Into::into)
    }
}

/// A generic, platform-agnostic Pubky Client.
///
/// This client contains the core business logic and is generic over an `HttpClient`
/// implementation, allowing it to operate in any environment (native, WASM, test).
#[derive(Clone, Debug)]
pub struct BaseClient<H: HttpClient> {
    /// The abstract HTTP client for making network requests.
    pub http: H,
    /// The client for interacting with the Pkarr DHT.
    pub pkarr: pkarr::Client,
    /// The record age threshold before republishing.
    pub max_record_age: Duration,
}

impl<H: HttpClient> BaseClient<H> {
    /// Creates a new `BaseClient` by injecting its dependencies: a platform-specific
    /// HTTP implementation and a configured Pkarr client.
    pub fn new(
        http_client: H,
        pkarr_client: pkarr::Client,
        max_record_age: Option<Duration>,
    ) -> Self {
        Self {
            http: http_client,
            pkarr: pkarr_client,
            max_record_age: max_record_age.unwrap_or(Duration::from_secs(60 * 60)),
        }
    }

    /// Returns a reference to the internal Pkarr Client.
    pub fn pkarr(&self) -> &pkarr::Client {
        &self.pkarr
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    /// Error building Pkarr client.
    PkarrBuildError(#[from] pkarr::errors::BuildError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_client::HttpClient;
    use anyhow::Result;
    use async_trait::async_trait;
    use reqwest::{Method, Url, header::HeaderMap};
    use std::sync::{Arc, Mutex};

    /// A mock HTTP client for testing.
    #[derive(Clone, Default)]
    struct MockHttpClient {
        last_called_url: Arc<Mutex<Option<Url>>>,
    }

    #[async_trait]
    impl HttpClient for MockHttpClient {
        async fn request(
            &self,
            _method: Method,
            url: Url,
            _body: Option<Vec<u8>>,
            _headers: Option<HeaderMap>,
        ) -> Result<Vec<u8>> {
            *self.last_called_url.lock().unwrap() = Some(url);
            Ok(b"mock response".to_vec())
        }
    }

    #[tokio::test]
    async fn test_get_rewrites_pubky_scheme() {
        // 1. Arrange
        let mock_http = MockHttpClient::default();
        let last_url = mock_http.last_called_url.clone();

        let client = BaseClient {
            http: mock_http,
            pkarr: pkarr::ClientBuilder::default()
                .build()
                .expect("should build"), // A default pkarr client is fine for this test.
            max_record_age: Duration::from_secs(3600),
        };

        let pkarr_key = pkarr::Keypair::random().public_key().to_string();
        let pubky_url = format!("pubky://{}/path", pkarr_key);
        let expected_https_url = format!("https://_pubky.{}/path", pkarr_key);

        // 2. Act
        let result = client.get(&pubky_url).await.unwrap();

        // 3. Assert
        assert_eq!(result, b"mock response".to_vec());
        let called_url = last_url.lock().unwrap().clone().unwrap();
        assert_eq!(called_url.as_str(), expected_https_url);
    }
}
