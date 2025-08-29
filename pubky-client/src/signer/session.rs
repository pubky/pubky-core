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
    /// Signup to a homeserver and publish `_pubky` pkarr record.
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<Session> {
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

        let response = check_http_status(response).await?;

        self.publish_homeserver_force(Some(&homeserver)).await?;

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }

    // pub async fn signup_into_agent(
    //     self,
    //     homeserver: &PublicKey,
    //     signup_token: Option<&str>,
    // ) -> Result<PubkyAgent<Keyless>> {
    // }

    // All of these methods use root capabilities

    /// Signin by locally signing an AuthToken.
    pub async fn signin(&self) -> Result<PubkyAgent> {
        self.signin_and_ensure_record_published(false).await
    }

    /// Signin and publish `_pubky` if stale.
    pub async fn signin_and_publish(&self) -> Result<PubkyAgent> {
        self.signin_and_ensure_record_published(true).await
    }

    async fn signin_and_ensure_record_published(&self, publish_sync: bool) -> Result<PubkyAgent> {
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let token = AuthToken::sign(&self.keypair, capabilities);
        let agent = PubkyAgent::new(self.client.clone(), &token).await?;

        if publish_sync {
            self.publish_homeserver_if_stale(None).await?;
        } else {
            // Fire-and-forget path: refresh in the background
            let agent = self.clone();
            let fut = async move {
                let _ = agent.publish_homeserver_if_stale(None).await;
            };
            #[cfg(not(target_arch = "wasm32"))]
            tokio::spawn(fut);
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(fut);
        };

        Ok(agent)
    }
}
