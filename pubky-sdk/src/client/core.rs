use std::fmt::Debug;
use std::time::Duration;

use crate::errors::BuildError;

const DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Clone)]
#[must_use]
/// Configures a [`PubkyHttpClient`] before construction.
///
/// Customize timeouts, user-agent, pkarr relays, and (WASM) testnet behavior.
/// Most code obtains this via [`PubkyHttpClient::builder()`], which simply returns
/// `PubkyHttpClientBuilder::default()`.
///
/// # Defaults
/// - Pkarr relays: [`crate::DEFAULT_RELAYS`]
/// - HTTP request timeout: reqwest default (no global timeout) unless set via
///   [`Self::request_timeout`]
/// - User-agent: `pubky.org@<crate-version>` plus any [`Self::user_agent_extra`]
///
/// # Example
/// ```no_run
/// use std::time::Duration;
/// # use pubky::{PubkyHttpClient, PubkyHttpClientBuilder};
/// let client = PubkyHttpClient::builder()
///     .request_timeout(Duration::from_secs(10))
///     .user_agent_extra("myapp/1.2.3")
///     .build()?;
/// # Ok::<_, pubky::BuildError>(())
/// ```
///
/// You can keep the default Pkarr relays or override them via the builder:
/// ```
/// # use pubky::{PubkyHttpClient, PubkyHttpClientBuilder};
/// # fn main() -> Result<(), pubky::BuildError> {
/// // Start from defaults; you can also supply your own entirely.
/// let mut b = PubkyHttpClient::builder();
/// b.pkarr(|p| p.relays(&["https://pkarr.example.net/"]).expect("infallible"));
/// let _client = b.build()?;
/// # Ok(()) }
/// ```
#[derive(Default)]
pub struct PubkyHttpClientBuilder {
    pkarr: pkarr::ClientBuilder,
    http_request_timeout: Option<Duration>,

    /// Optional user-agent segment appended to the default UA for app-level telemetry.
    user_agent_extra: Option<String>,

    /// The hostname to use for testnet URL transformations (WASM only).
    #[cfg(target_arch = "wasm32")]
    testnet_host: Option<String>,
}

impl PubkyHttpClientBuilder {
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
    ///
    /// Use this to influence PKARR resolution inputs (relays, bootstrap nodes,
    /// timeouts, etc.) *before* building the client. There are no per-request
    /// resolution knobs; configuration is done up front.
    ///
    /// # Example
    /// ```
    /// # use pubky::{PubkyHttpClient, PubkyHttpClientBuilder};
    /// let client = PubkyHttpClient::builder()
    ///     .pkarr(|p| p
    ///         .relays(&["https://pkarr.example.net/"]).expect("infallible")
    ///         .bootstrap(&["dht.node.example:6881"])
    ///     )
    ///     .build()?;
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
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

    /// Build [PubkyHttpClient]
    pub fn build(&self) -> Result<PubkyHttpClient, BuildError> {
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
        Ok(PubkyHttpClient {
            pkarr,
            http: http_builder.build()?,

            #[cfg(not(target_arch = "wasm32"))]
            icann_http: icann_http_builder.build()?,

            #[cfg(target_arch = "wasm32")]
            testnet_host: self.testnet_host.clone(),
        })
    }
}

/// Transport client for Pubky homeserver APIs and generic HTTP, with PKARR-aware
/// URL handling.
///
/// `PubkyHttpClient` is the low-level, stateless engine the higher-level actors
/// (`PubkyAgent`, `PubkyDrive`, `Pkdns`, `PubkyPairingAuth`) are built on. It owns:
/// - A pkarr DHT client (for resolving pkdns endpoints and publishing records).
/// - One or more reqwest HTTP clients (platform-specific).
///
/// ### What it does
/// - Understands `pubky://<user>/<path>` and rewrites it to the correct HTTPS
///   form for requests.
/// - Detects pkarr public-key hosts and resolves them to concrete endpoints.
/// - Internally, uses a unified `cross_request(..)` that works the same on native rust and
///   WASM (WASM performs endpoint resolution & header injection; native is a thin wrapper).
///
/// ### What it *doesn’t* do
/// - It is **not** session/identity aware. No cookies, no per-user scoping.
///   For authenticated per-user flows use [`crate::PubkyAgent`].
///
/// ### When to use
/// - You want direct control over the PubkyHttpClient (power users, libs).
/// - You’re wiring custom flows/tests and don’t need the high-level ergonomics.
///
/// For most apps, prefer the higher-level actors and let them reuse the default shared
/// [`crate::global::global_client`] under the hood.
///
/// ### Construction
/// Use [`PubkyHttpClient::builder()`] to tweak timeouts, relays, or
/// user-agent; or pick sensible defaults via [`PubkyHttpClient::new()`]. A
/// [`PubkyHttpClient::testnet()`] helper configures a local test network.
///
/// ### Platform notes
/// - **Native (rust, not WASM target):**
///   - ICANN domains use standard X.509 TLS via the `icann_http` client.
///   - Pubky/PKDNS hosts (public-key hostnames / `pubky://…`) use **PubkyTLS**
///     (TLS with RFC 7250 Raw Public Keys), verifying the connection against the
///     target public key—no CA chain involved.
/// - **WASM:**
///   - All requests use the browser’s standard X.509 TLS stack.
///   - For Pubky/PKDNS hosts, private method `cross_request(..)` resolves the
///     endpoint via PKARR, rewrites the URL (including testnet/localhost mapping),
///     and may add a `pubky-host` header to convey the intended public-key host.
///
/// ### Examples
/// Basic construction. Works out of the box for mainline DHT pkarr endpoints.
/// ```no_run
/// # use pubky::PubkyHttpClient;
/// let client = PubkyHttpClient::new()?;
/// # Ok::<_, pubky::BuildError>(())
/// ```
///
/// Fetching a standard ICANN URL or any URL with `request`:
/// ```no_run
/// # use pubky::{PubkyHttpClient, Result};
/// # use reqwest::Method;
/// # async fn run() -> Result<()> {
/// let client = PubkyHttpClient::new()?;
/// let resp = client.request(Method::GET, "https://example.com").await?
///     .send().await?;
/// assert!(resp.status().is_success());
/// # Ok(()) }
/// ```
///
/// Resolving and fetching a `pubky://` resource directly:
/// ```no_run
/// # use pubky::{PubkyHttpClient, Result};
/// # use reqwest::Method;
/// # async fn run(user: &str) -> Result<()> {
/// let client = PubkyHttpClient::new()?;
/// let url = format!("pubky://{}/pub/app/info.json", user);
/// let resp = client.request(Method::GET, &url).await?
///     .send().await?;
/// let info = resp.text().await?;
/// # Ok(()) }
/// ```
///
/// > Tip: For authenticated reads/writes, prefer `agent.drive().get(...)`, which
/// > automatically scopes paths and attaches the right session cookie.
#[derive(Clone, Debug)]
pub struct PubkyHttpClient {
    pub(crate) http: reqwest::Client,
    pub(crate) pkarr: pkarr::Client,

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) icann_http: reqwest::Client,

    /// The hostname to use for testnet URL transformations (WASM only).
    #[cfg(target_arch = "wasm32")]
    pub(crate) testnet_host: Option<String>,
}

impl PubkyHttpClient {
    /// Creates a client configured for public mainline DHT and pkarr relays.
    pub fn new() -> Result<PubkyHttpClient, BuildError> {
        Self::builder().build()
    }

    /// Returns a builder to edit settings before creating [`PubkyHttpClient`].
    /// Prefer this when you need to control PKARR/DHT inputs (relays, bootstrap);
    /// resolution itself remains automatic during requests.
    pub fn builder() -> PubkyHttpClientBuilder {
        PubkyHttpClientBuilder::default()
    }

    /// Creates a client configured to use testnet DHT and Pkarr relays running on `localhost`.
    /// You need an instance of `pubky-testnet` running on `localhost`
    pub fn testnet() -> Result<PubkyHttpClient, BuildError> {
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
        let client = PubkyHttpClient::new().unwrap();
        let response = client
            .request(Method::GET, "https://example.com/")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
}
