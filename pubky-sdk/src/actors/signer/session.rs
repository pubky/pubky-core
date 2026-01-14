use pubky_common::auth::AuthToken;
use reqwest::Method;
use url::Url;

use super::PubkySigner;
use crate::{
    Capabilities, Capability, PubkySession, PublicKey, Result, cross_log, util::check_http_status,
};

#[derive(Debug, Clone, Copy)]
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
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Parse`] if the homeserver URL cannot be constructed.
    /// - Propagates transport failures while creating the account or publishing the homeserver record.
    /// - Propagates validation errors from the session hydration step.
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<PubkySession> {
        let url = Self::build_signup_url(homeserver, signup_token)?;
        cross_log!(info, "Signing up new account on homeserver {}", homeserver);

        let auth_token = self.root_capability_token();
        let response = self
            .send_signup_request(url, auth_token.serialize())
            .await?;

        self.publish_signup_homeserver(homeserver).await?;
        PubkySession::new_from_response(self.client.clone(), response).await
    }

    // All of these methods use root capabilities

    /// Sign in to the users homeserver by locally signing a root-capability token.
    /// This call returns a user session.
    ///
    /// In case the users pkdns records are stale, this call with republish them in the background.
    ///
    /// Prefer this signin for best user experience, it returns fast.
    ///
    /// # Errors
    /// - Propagates transport failures during the session exchange.
    /// - Propagates validation errors from the session exchange or PKDNS publishing.
    pub async fn signin(&self) -> Result<PubkySession> {
        self.signin_with_publish(PublishMode::Background).await
    }

    /// Sign in by locally signing a root-capability token. Returns a session-bound session.
    /// Publishes the homeserver record if stale in the background.
    ///
    /// Prefer this signin for highest guarantees of discoverability from Dht and pkarr relays,
    /// it returns slow (~3-5 seconds).
    ///
    /// # Errors
    /// - Propagates transport failures during the session exchange.
    /// - Propagates validation errors from the session exchange or PKDNS publishing.
    pub async fn signin_blocking(&self) -> Result<PubkySession> {
        self.signin_with_publish(PublishMode::Blocking).await
    }

    /// Internal helper to sign in, then optionally refresh `_pubky` record.
    async fn signin_with_publish(&self, mode: PublishMode) -> Result<PubkySession> {
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let token = AuthToken::sign(&self.keypair, capabilities);
        let session = PubkySession::new(&token, self.client.clone()).await?;
        cross_log!(
            info,
            "Signin completed for {}; mode {:?}",
            self.keypair.public_key(),
            mode
        );

        match mode {
            PublishMode::Blocking => {
                cross_log!(
                    info,
                    "Publishing homeserver for {} in blocking mode",
                    self.keypair.public_key()
                );
                self.pkdns().publish_homeserver_if_stale(None).await?;
            }
            PublishMode::Background => {
                // Fire-and-forget path: refresh in the background
                let signer = self.clone();
                let fut = async move {
                    cross_log!(
                        info,
                        "Background publish of homeserver for {} started",
                        signer.keypair.public_key()
                    );
                    let _ = signer.pkdns().publish_homeserver_if_stale(None).await;
                    cross_log!(
                        info,
                        "Background publish task for {} completed",
                        signer.keypair.public_key()
                    );
                };
                #[cfg(not(target_arch = "wasm32"))]
                tokio::spawn(fut);
                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_futures::spawn_local(fut);
            }
        }

        Ok(session)
    }

    fn build_signup_url(homeserver: &PublicKey, signup_token: Option<&str>) -> Result<Url> {
        let mut url = Url::parse(&format!("https://{}", homeserver.z32()))?;
        url.set_path("/signup");
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }
        Ok(url)
    }

    fn root_capability_token(&self) -> AuthToken {
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        AuthToken::sign(&self.keypair, capabilities)
    }

    async fn send_signup_request(&self, url: Url, body: Vec<u8>) -> Result<reqwest::Response> {
        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(body)
            .send()
            .await?;

        // Map non-2xx into our error type; keep body/headers intact for the caller.
        check_http_status(response).await
    }

    async fn publish_signup_homeserver(&self, homeserver: &PublicKey) -> Result<()> {
        cross_log!(
            info,
            "Signup request for {} succeeded; publishing homeserver",
            self.keypair.public_key()
        );

        self.pkdns()
            .publish_homeserver_force(Some(homeserver))
            .await?;

        cross_log!(
            info,
            "Signup homeserver publish complete for {}",
            self.keypair.public_key()
        );
        Ok(())
    }
}
