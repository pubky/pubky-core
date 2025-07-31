use super::cookies::CookieJar;
use super::http_client::NativeHttpClient;
use crate::{BaseClient, BuildError, ClientConfig, DEFAULT_USER_AGENT};
use std::sync::Arc;

/// A type alias for the native-specific Pubky client, for convenience.
pub type Client = BaseClient<NativeHttpClient>;

/// Implementation block providing convenient constructors for the `Client`.
impl Client {
    /// Returns a default configuration object for the native client.
    pub fn config() -> ClientConfig {
        ClientConfig::new()
    }

    /// Creates a new native client from a `ClientConfig` object.
    /// This is the final assembly step, containing all native-specific wiring.
    pub fn from_config(config: ClientConfig) -> Result<Self, BuildError> {
        // 1. Build the pkarr::Client from the configuration.
        let pkarr_client = config.build_pkarr_client()?;

        // 2. Construct the native-specific reqwest clients.
        let cookie_store = Arc::new(CookieJar::default());

        let pkarr_http = reqwest::ClientBuilder::from(pkarr_client.clone())
            .cookie_provider(cookie_store.clone())
            .user_agent(DEFAULT_USER_AGENT)
            .build()?;

        let icann_http = reqwest::Client::builder()
            .cookie_provider(cookie_store.clone())
            .user_agent(DEFAULT_USER_AGENT)
            .build()?;

        // 3. Assemble the concrete `NativeHttpClient`.
        let native_http_client = NativeHttpClient {
            pkarr_client: pkarr_http,
            icann_client: icann_http,
            cookie_store: cookie_store,
        };

        // 4. Create the final generic `Client` instance using the universal constructor.
        Ok(BaseClient::new(
            native_http_client,
            pkarr_client,
            config.max_record_age,
        ))
    }

    /// Creates a client connected to a local test network using "localhost".
    ///
    /// For a custom hostname, see `testnet_with_host`.
    pub fn testnet() -> Result<Self, BuildError> {
        Self::testnet_with_host("localhost")
    }

    /// Creates a client connected to a local test network with a specific hostname.
    pub fn testnet_with_host(host: &str) -> Result<Self, BuildError> {
        let mut config = Self::config();
        config.pkarr(|pkarr| {
            pkarr
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
                .expect("relays urls infallible")
        });
        Self::from_config(config)
    }
}

impl Default for Client {
    /// Returns a Native Pubky Client with default configuration.
    fn default() -> Self {
        Self::from_config(ClientConfig::new())
            .expect("Default Pubky native client should have valid config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_native_client_fetches_icann_domain() -> Result<()> {
        // 1. Arrange: Create a real NativeClient.
        // This uses the actual reqwest-based NativeHttpClient internally.
        let client = BaseClient::default();

        // 2. Act: Make a real network request to an ICANN domain.
        let response = client.get("https://google.com").await?;

        // 3. Assert: Check that the request was successful and returned a non-empty body.
        // A successful get from google.com should always have content.
        assert!(
            !response.body.is_empty(),
            "Response body from google.com should not be empty"
        );

        Ok(())
    }
}
