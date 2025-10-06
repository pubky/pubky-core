use wasm_bindgen::prelude::*;

use super::{pkdns::Pkdns, session::Session};
use crate::js_error::JsResult;
use crate::wrappers::{keys::Keypair, keys::PublicKey};

/// Holds a user’s `Keypair` and performs identity operations:
/// - `signup` creates a new homeserver user.
/// - `signin` creates a homeserver session for an existing user.
/// - Approve pubkyauth requests
/// - Publish PKDNS when signer-bound
#[wasm_bindgen]
pub struct Signer(pub(crate) pubky::PubkySigner);

#[wasm_bindgen]
impl Signer {
    /// Create a signer from a `Keypair` (prefer `pubky.signer(kp)`).
    ///
    /// @param {Keypair} keypair
    /// @returns {Signer}
    #[wasm_bindgen(js_name = "fromKeypair")]
    pub fn new(keypair: &Keypair) -> Signer {
        let signer = pubky::PubkySigner::new(keypair.as_inner().clone())
            .expect("Signer construction should not fail with a valid keypair");
        Signer(signer)
    }

    /// Get the public key of this signer.
    ///
    /// @returns {PublicKey}
    #[wasm_bindgen(js_name = "publicKey", getter)]
    pub fn public_key(&self) -> PublicKey {
        self.0.public_key().into()
    }

    /// Sign up at a homeserver. Returns a ready `Session`.
    ///
    /// Creates a valid homeserver Session with root capabilities
    ///
    /// @param {PublicKey} homeserver The homeserver’s public key.
    /// @param {string|null} signupToken Invite/registration token or `null`.
    /// @returns {Promise<Session>}
    ///
    /// @throws {PubkyError}
    /// - `AuthenticationError` (bad/expired token)
    /// - `RequestError` (network/server)
    #[wasm_bindgen]
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<String>,
    ) -> JsResult<Session> {
        let session = self
            .0
            .signup(homeserver.as_inner(), signup_token.as_deref())
            .await?;
        Ok(Session(session))
    }

    /// Fast sign-in for a returning user. Publishes PKDNS in the background.
    ///
    /// Creates a valid homeserver Session with root capabilities
    ///
    /// @returns {Promise<Session>}
    ///
    /// @throws {PubkyError}
    #[wasm_bindgen]
    pub async fn signin(&self) -> JsResult<Session> {
        Ok(Session(self.0.signin().await?))
    }

    /// Blocking sign-in. Waits for PKDNS publish to complete (slower; safer).
    ///
    /// Creates a valid homeserver Session with root capabilities
    ///
    /// @returns {Promise<Session>}
    #[wasm_bindgen(js_name = "signinBlocking")]
    pub async fn signin_blocking(&self) -> JsResult<Session> {
        Ok(Session(self.0.signin_blocking().await?))
    }

    /// Approve a `pubkyauth://` request URL (encrypts & POSTs the signed AuthToken).
    #[wasm_bindgen(js_name = "approveAuthRequest")]
    pub async fn approve_auth(&self, pubkyauth_url: String) -> JsResult<()> {
        self.0.approve_auth(&pubkyauth_url).await?;
        Ok(())
    }

    /// Get a PKDNS actor bound to this signer's client & keypair (publishing enabled).
    ///
    /// @returns {Pkdns}
    ///
    /// @example
    /// await signer.pkdns.publishHomeserverIfStale(homeserverPk);
    #[wasm_bindgen(getter)]
    pub fn pkdns(&self) -> Pkdns {
        Pkdns(self.0.pkdns())
    }
}
