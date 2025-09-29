//! High-level façade for the Pubky crate.
//!
//! ## Mental model
//! - `Pubky` - your entrypoint/handle to the SDK. Owns a `PubkyHttpClient`.
//! - `Signer` - local private keys; can `signin`/`signup`, publish PKDNS, approve auth requests.
//! - `Session` - authenticated, “as me” API; exposes scoped storage.
//! - `PublicStorage` - unauthenticated, “read others” API.
//!
//! ## Quick starts
//! ### 1) App sign-in via QR/deeplink (auth flow)
//! ```no_run
//! use pubky::{Pubky, Capabilities};
//!
//! # async fn run() -> pubky::Result<()> {
//! let pubky = Pubky::new()?; // or Pubky::testnet() / Pubky::with_client(...)
//!
//! let caps = Capabilities::default();
//! let flow = pubky.start_auth_flow(&caps)?;
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ### 2) Script that holds a key and signs in locally with root capabilities
//! ```no_run
//! use pubky::{Pubky, PubkySigner, Keypair};
//!
//! # async fn run() -> pubky::Result<()> {
//! let pubky = Pubky::new()?;
//! let kp = Keypair::random();
//! let signer = pubky.signer(kp);
//!
//! let session = signer.signin().await?;
//! // do writes as-me
//! session.storage().put("/pub/demo/hello.txt", "hi").await?;
//! # Ok(()) }
//! ```
//!
//! ### 3) Public read (no identity)
//! ```no_run
//! use pubky::Pubky;
//!
//! # async fn run(user: pubky::PublicKey) -> pubky::Result<()> {
//! let pubky = Pubky::new()?;
//! let public = pubky.public_storage();
//! let addr = format!("{}/pub/site/index.html", user);
//! let html = public.get(addr).await?.text().await?;
//! # Ok(()) }
//! ```

use crate::{Capabilities, PubkyAuthFlow, PubkyHttpClient, PubkySigner, PublicStorage, Result};

/// High-level façade. Owns a `PubkyHttpClient` and constructs the main actors.
#[derive(Clone, Debug)]
pub struct Pubky {
    client: PubkyHttpClient,
}

impl Pubky {
    /// Construct with defaults (mainnet relays, standard timeouts).
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::new()?,
        })
    }

    /// Construct preconfigured for a local Pubky testnet.
    pub fn testnet() -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::testnet()?,
        })
    }

    /// Construct from an already-configured transport.
    pub fn with_client(client: PubkyHttpClient) -> Self {
        Self { client }
    }

    /// Start an end-to-end auth flow (QR/deeplink).
    ///
    /// Use with `flow.authorization_url()` and then `await_approval()` (blocking)
    /// or `try_poll_once()` (non-blocking UI loops).
    pub fn start_auth_flow(&self, caps: &Capabilities) -> Result<PubkyAuthFlow> {
        PubkyAuthFlow::builder(caps.clone())
            .client(self.client.clone())
            .start()
    }

    /// Create a `PubkySigner` for a given keypair.
    pub fn signer(&self, keypair: crate::Keypair) -> PubkySigner {
        PubkySigner {
            client: self.client.clone(),
            keypair,
        }
    }

    /// Create a `PubkySigner` with a fresh random keypair.
    pub fn signer_random(&self) -> PubkySigner {
        self.signer(crate::Keypair::random())
    }

    /// Create a public, unauthenticated storage handle using this façade’s client.
    pub fn public_storage(&self) -> PublicStorage {
        PublicStorage {
            client: self.client.clone(),
        }
    }

    /// Read-only PKDNS actor (resolve `_pubky` records) using this façade’s client.
    pub fn pkdns(&self) -> crate::Pkdns {
        crate::Pkdns::with_client(self.client.clone())
    }

    /// Access the underlying transport (advanced use).
    #[inline]
    pub fn client(&self) -> &PubkyHttpClient {
        &self.client
    }
}
