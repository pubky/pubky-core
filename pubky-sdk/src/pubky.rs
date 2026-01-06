//! High-level facade for the Pubky crate.
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
//! use pubky::{Pubky, Capabilities, AuthFlowKind};
//!
//! # async fn run() -> pubky::Result<()> {
//! let pubky = Pubky::new()?; // or Pubky::testnet() / Pubky::with_client(...)
//!
//! let caps = Capabilities::default();
//! let flow = pubky.start_auth_flow(&caps, AuthFlowKind::signin())?;
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
//! let addr = format!("pubky{}/pub/site/index.html", user);
//! let html = public.get(addr).await?.text().await?;
//! # Ok(()) }
//! ```

use pkarr::PublicKey;

use crate::{
    Capabilities, Pkdns, PubkyAuthFlow, PubkyHttpClient, PubkySigner, PublicStorage, Result,
    actors::AuthFlowKind,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::{PubkySession, errors::RequestError};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// High-level facade. Owns a `PubkyHttpClient` and constructs the main actors.
/// Prefer to instantiate only once and use trough your application a single shared `Pubky`
/// instead of constructing one per request. This avoids reinitializing transports and keeps
/// the same client available for repeated usage.
#[derive(Clone, Debug)]
pub struct Pubky {
    client: PubkyHttpClient,
}

impl Pubky {
    /// Construct with defaults (mainnet relays, standard timeouts).
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] when the underlying [`PubkyHttpClient`] fails to
    ///   initialize (e.g., TLS configuration or relay/bootstrap setup issues).
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::new()?,
        })
    }

    /// Construct preconfigured for a local Pubky testnet.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] when the testnet-configured [`PubkyHttpClient`]
    ///   cannot be created (for example, invalid local relay/testnet configuration).
    pub fn testnet() -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::testnet()?,
        })
    }

    /// Construct from an already-configured transport.
    #[must_use]
    pub const fn with_client(client: PubkyHttpClient) -> Self {
        Self { client }
    }

    /// Start an end-to-end auth flow (QR/deeplink).
    /// Depending on the auth kind, the flow will be different.
    /// - `AuthFlowKind::SignIn` - Sign in to an existing account.
    /// - `AuthFlowKind::SignUp` - Sign up for a new account.
    ///
    /// Use with `flow.authorization_url()` and then `await_approval()` (blocking)
    /// or `try_poll_once()` (non-blocking UI loops).
    ///
    /// # Errors
    /// - [`crate::errors::Error::Parse`] if internal URL construction for the flow
    ///   fails (e.g., malformed relay URL when configured via the builder).
    pub fn start_auth_flow(
        &self,
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
    ) -> Result<PubkyAuthFlow> {
        PubkyAuthFlow::builder(caps, auth_kind)
            .client(self.client.clone())
            .start()
    }

    /// Create a `PubkySigner` for a given keypair.
    #[must_use]
    pub fn signer(&self, keypair: crate::Keypair) -> PubkySigner {
        PubkySigner {
            client: self.client.clone(),
            keypair,
        }
    }

    /// Create a public, unauthenticated storage handle using this facade’s client.
    #[must_use]
    pub fn public_storage(&self) -> PublicStorage {
        PublicStorage {
            client: self.client.clone(),
        }
    }

    /// Read-only [`Pkdns`] actor (resolve `_pubky` records) using this facade’s client.
    #[must_use]
    pub fn pkdns(&self) -> Pkdns {
        Pkdns::with_client(self.client.clone())
    }

    /// Resolve current homeserver host for a user public key via Pkarr.
    ///
    /// Returns the `_pubky` SVCB/HTTPS target (domain or pubkey-as-host),
    /// or `None` if the record is missing/unresolvable. Uses an internal
    /// read-only [`Pkdns`] actor.
    pub async fn get_homeserver_of(&self, user_public_key: &PublicKey) -> Option<PublicKey> {
        Pkdns::with_client(self.client.clone())
            .get_homeserver_of(user_public_key)
            .await
    }

    // ------ Persistance helpers ----------

    /// Restore a session from a `.sess` secret file.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Request`] if the secret file cannot be read.
    /// - Returns [`crate::errors::RequestError::Validation`] when the file contents are malformed.
    /// - Propagates transport errors from [`PubkySession::from_secret_file`] if the client
    ///   cannot be prepared.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn session_from_file<P: AsRef<Path>>(&self, path: P) -> Result<PubkySession> {
        PubkySession::from_secret_file(path.as_ref(), Some(self.client.clone())).await
    }

    /// Recover a keypair from an encrypted `.pkarr` secret file and return a [`PubkySigner`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Request`] when reading the recovery file fails.
    /// - Returns [`crate::errors::Error::Request`] when decryption fails (invalid passphrase or corrupted file).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn signer_from_recovery_file<P: AsRef<Path>>(
        &self,
        path: P,
        passphrase: &str,
    ) -> Result<PubkySigner> {
        use pubky_common::recovery_file::decrypt_recovery_file;

        let bytes = std::fs::read(path.as_ref()).map_err(|e| RequestError::Validation {
            message: format!("failed to read recovery file: {e}"),
        })?;

        let kp =
            decrypt_recovery_file(&bytes, passphrase).map_err(|e| RequestError::Validation {
                message: format!("failed to decrypt recovery file: {e}"),
            })?;

        Ok(self.signer(kp))
    }

    /// Access the underlying transport (advanced use).
    #[inline]
    #[must_use]
    pub const fn client(&self) -> &PubkyHttpClient {
        &self.client
    }
}
