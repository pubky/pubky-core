use wasm_bindgen::prelude::*;

use super::{pkdns::Pkdns, session::Session};
use crate::js_result::JsResult;
use crate::wrappers::{keys::Keypair, keys::PublicKey};

#[wasm_bindgen]
pub struct Signer(pub(crate) pubky::PubkySigner);

#[wasm_bindgen]
impl Signer {
    /// Construct a Signer from a Keypair.
    #[wasm_bindgen(js_name = "fromKeypair")]
    pub fn new(keypair: &Keypair) -> Signer {
        let signer = pubky::PubkySigner::new(keypair.as_inner().clone())
            .expect("Signer construction should not fail with a valid keypair");
        Signer(signer)
    }

    /// Construct a signer with a fresh random keypair (for ephemeral tests).
    #[wasm_bindgen(js_name = "random")]
    pub fn random() -> JsResult<Signer> {
        Ok(Signer(pubky::PubkySigner::random()?))
    }

    /// Return the signer's PublicKey.
    #[wasm_bindgen(js_name = "publicKey")]
    pub fn public_key(&self) -> PublicKey {
        self.0.public_key().into()
    }

    /// Get a PKDNS actor bound to this signer's client & keypair (publishing enabled).
    #[wasm_bindgen]
    pub fn pkdns(&self) -> Pkdns {
        Pkdns(self.0.pkdns())
    }

    /// Sign up at a homeserver and return a ready `Session`.
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

    /// Sign in (fast; publishes homeserver record in background if stale).
    #[wasm_bindgen]
    pub async fn signin(&self) -> JsResult<Session> {
        Ok(Session(self.0.signin().await?))
    }

    /// Sign in, blocking until homeserver record publish completes.
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
}
