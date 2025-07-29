use super::http_client::HttpClient;
use std::{fmt::Debug, time::Duration};

#[cfg(not(target_arch = "wasm32"))]
use super::internal::cookies::CookieJar;
#[cfg(not(target_arch = "wasm32"))]
use super::native_http_client::NativeHttpClient;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

// Static constants remain unchanged.
static DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"));
static DEFAULT_RELAYS: &[&str] = &["https://pkarr.pubky.org/", "https://pkarr.pubky.app/"];

/// Holds the platform-agnostic configuration for a `Client`.
///
/// This struct is used to configure and build the necessary components (like `pkarr::Client`)
/// before they are combined with a platform-specific `HttpClient` to create a full `Client`.
#[derive(Debug, Default, Clone)]
pub struct ClientConfig {
    pkarr: pkarr::ClientBuilder,
    max_record_age: Option<Duration>,
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

/// A generic, platform-agnostic client for Pubky APIs.
///
/// This client contains the core business logic and is generic over an `HttpClient`
/// implementation, allowing it to operate in any environment (native, WASM, test).
#[derive(Clone, Debug)]
pub struct Client<H: HttpClient> {
    /// The abstract HTTP client for making network requests.
    pub http: H,
    /// The client for interacting with the Pkarr DHT.
    pub pkarr: pkarr::Client,
    /// The record age threshold before republishing.
    pub max_record_age: Duration,
}

impl<H: HttpClient> Client<H> {
    /// Creates a new `Client` by injecting its dependencies: a platform-specific
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

/// A type alias for the native-specific Pubky client, for convenience.
#[cfg(not(target_arch = "wasm32"))]
pub type NativeClient = Client<NativeHttpClient>;

/// Implementation block providing convenient constructors for the `NativeClient`.
#[cfg(not(target_arch = "wasm32"))]
impl NativeClient {
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
            .build()
            .expect("Native pkarr reqwest client build should not fail");

        let icann_http = reqwest::Client::builder()
            .cookie_provider(cookie_store.clone())
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .expect("Native icann reqwest client build should not fail");

        // 3. Assemble the concrete `NativeHttpClient`.
        let native_http_client = NativeHttpClient {
            pkarr_client: pkarr_http,
            icann_client: icann_http,
            cookie_store: cookie_store,
        };

        // 4. Create the final generic `Client` instance using the universal constructor.
        Ok(Client::new(
            native_http_client,
            pkarr_client,
            config.max_record_age,
        ))
    }

    /// A convenience method to create a client connected to a local test network.
    pub fn testnet(host: &str) -> Result<Self, BuildError> {
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

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    /// Error building Pkarr client.
    PkarrBuildError(#[from] pkarr::errors::BuildError),
}
