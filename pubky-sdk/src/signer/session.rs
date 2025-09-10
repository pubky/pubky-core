use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    session::Session,
};
use reqwest::Method;
use url::Url;

use crate::{PubkyAgent, PublicKey, Result, util::check_http_status};

use super::PubkySigner;

impl PubkySigner {
    /// Create an account on the given homeserver and return the parsed `Session`.
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
    ) -> Result<Session> {
        let response = self.post_signup(homeserver, signup_token).await?;

        // Keep behavior consistent with the previous version: publish before returning.
        self.pkdns()
            .publish_homeserver_force(Some(homeserver))
            .await?;

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }

    /// Create an account on a homeserver and return a ready-to-use `PubkyAgent`.
    ///
    /// Prefer this when you want to start acting as the user immediately after signup.
    pub async fn signup_agent(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<PubkyAgent> {
        let response = self.post_signup(homeserver, signup_token).await?;
        self.pkdns()
            .publish_homeserver_force(Some(homeserver))
            .await?;
        PubkyAgent::new_from_response(self.client.clone(), response).await
    }

    /// POST `https://<homeserver>/signup` with a root-capability token and return the checked response.
    async fn post_signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<reqwest::Response> {
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
        check_http_status(response).await
    }

    // All of these methods use root capabilities

    /// Sign in by locally signing a root-capability token. Returns a session-bound agent.
    pub async fn signin(&self) -> Result<PubkyAgent> {
        self.signin_and_ensure_record_published(false).await
    }

    /// Signin and publish `_pubky` if stale in sync.
    pub async fn signin_and_publish(&self) -> Result<PubkyAgent> {
        self.signin_and_ensure_record_published(true).await
    }

    async fn signin_and_ensure_record_published(&self, publish_sync: bool) -> Result<PubkyAgent> {
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let token = AuthToken::sign(&self.keypair, capabilities);
        let agent = PubkyAgent::new(&self.client, &token).await?;

        if publish_sync {
            self.pkdns().publish_homeserver_if_stale(None).await?;
        } else {
            // Fire-and-forget path: refresh in the background
            let agent = self.clone();
            let fut = async move {
                let _ = agent.pkdns().publish_homeserver_if_stale(None).await;
            };
            #[cfg(not(target_arch = "wasm32"))]
            tokio::spawn(fut);
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(fut);
        };

        Ok(agent)
    }
}
