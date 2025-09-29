use pubky_common::auth::AuthToken;
use reqwest::Method;
use url::Url;

use super::PubkySigner;
use crate::{Capabilities, Capability, PubkySession, PublicKey, Result, util::check_http_status};

enum PublishMode {
    Background,
    Blocking,
}

impl PubkySigner {
    /// Create an account on a homeserver and return a ready-to-use `PubkySession`.
    ///
    /// Side effects:
    /// - Publishes the `_pubky` pkarr record pointing to `homeserver` (force mode).
    ///
    /// Notes:
    /// - Uses a **root** capability token (sufficient for signup).
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<PubkySession> {
        let mut url = Url::parse(&format!("https://{}", homeserver))?;
        url.set_path("/signup");
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }

        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let auth_token = AuthToken::sign(&self.keypair, capabilities);

        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(auth_token.serialize())
            .send()
            .await?;

        // Map non-2xx into our error type; keep body/headers intact for the caller.
        let response = check_http_status(response).await?;

        self.pkdns()
            .publish_homeserver_force(Some(homeserver))
            .await?;
        PubkySession::new_from_response(self.client.clone(), response).await
    }

    // All of these methods use root capabilities

    /// Sign in to the users homeserver by locally signing a root-capability token.
    /// This call returns a user session.
    ///
    /// In case the users pkdns records are stale, this call with republish them in the background.
    ///
    /// Prefer this signin for best user experience, it returns fast.
    pub async fn signin(&self) -> Result<PubkySession> {
        self.signin_with_publish(PublishMode::Background).await
    }

    /// Sign in by locally signing a root-capability token. Returns a session-bound session.
    /// Publishes the homeserver record if stale in the background.
    ///
    /// Prefer this signin for highest guarantees of discoverability from Dht and pkarr relays,
    /// it returns slow (~3-5 seconds).
    pub async fn signin_blocking(&self) -> Result<PubkySession> {
        self.signin_with_publish(PublishMode::Blocking).await
    }

    /// Internal helper to sign in, then optionally refresh `_pubky` record.
    async fn signin_with_publish(&self, mode: PublishMode) -> Result<PubkySession> {
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let token = AuthToken::sign(&self.keypair, capabilities);
        let session = PubkySession::new(&token).await?;

        match mode {
            PublishMode::Blocking => {
                self.pkdns().publish_homeserver_if_stale(None).await?;
            }
            PublishMode::Background => {
                // Fire-and-forget path: refresh in the background
                let signer = self.clone();
                let fut = async move {
                    let _ = signer.pkdns().publish_homeserver_if_stale(None).await;
                };
                #[cfg(not(target_arch = "wasm32"))]
                tokio::spawn(fut);
                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_futures::spawn_local(fut);
            }
        }

        Ok(session)
    }
}
