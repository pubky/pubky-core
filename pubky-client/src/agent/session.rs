use reqwest::{Method, Response, StatusCode};

use url::Url;

use pkarr::PublicKey;
use pubky_common::{auth::AuthToken, capabilities::Capability, session::Session};

use crate::{
    PubkyAgent,
    agent::state::{Keyed, sealed::Sealed},
    client::pkarr::PublishStrategy,
    errors::Result,
    util::check_http_status,
};

impl<S: Sealed> PubkyAgent<S> {
    /// Retrieve session for current pubky. Fails if pubky is unknown.
    pub async fn session(&self) -> Result<Option<Session>> {
        let response = self.homeserver().get("/session").await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Signout from homeserver and clear this agent’s cookie.
    pub async fn signout(&self) -> Result<()> {
        let response = self.homeserver().delete("/session").await?;
        check_http_status(response).await?;

        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut slot) = self.session_secret.write() {
            *slot = None;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_session_cookie_for(&self, response: &Response, pubky: &PublicKey) {
        use reqwest::header::SET_COOKIE;
        let cookie_name = pubky.to_string();

        for (name, val) in response.headers().iter() {
            if name == SET_COOKIE {
                if let Ok(v) = std::str::from_utf8(val.as_bytes()) {
                    if let Ok(parsed) = cookie::Cookie::parse(v.to_owned()) {
                        if parsed.name() == cookie_name {
                            if let Ok(mut slot) = self.session_secret.write() {
                                *slot = Some(parsed.value().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_session_cookie(&self, response: &Response) -> Result<()> {
        let pk = self.require_pubky()?;
        self.capture_session_cookie_for(response, &pk);
        Ok(())
    }
}

impl PubkyAgent<Keyed> {
    /// Signup to a homeserver and publish `_pubky` record. Requires a keypair.
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<Session> {
        let kp = self.keypair.get();

        let mut url = Url::parse(&format!("https://{}", homeserver))?;
        url.set_path("/signup");
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }

        let auth_token = AuthToken::sign(kp, vec![Capability::root()]);
        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(auth_token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;

        self.client
            .publish_homeserver(kp, Some(&homeserver.to_string()), PublishStrategy::Force)
            .await?;

        // On successful signup, the agent’s pubky is known from the keypair.
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(kp.public_key());
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie_for(&response, &kp.public_key());

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }

    /// Signin by locally signing an AuthToken. Requires keypair.
    pub async fn signin(&self) -> Result<Session> {
        self.signin_and_ensure_record_published(false).await
    }

    /// Signin and publish `_pubky` if stale. Requires keypair.
    pub async fn signin_and_publish(&self) -> Result<Session> {
        self.signin_and_ensure_record_published(true).await
    }

    async fn signin_and_ensure_record_published(&self, publish_sync: bool) -> Result<Session> {
        let kp = self.keypair.get();

        let token = AuthToken::sign(kp, vec![Capability::root()]);
        let session = self.signin_with_authtoken(&token).await?;

        if publish_sync {
            self.client
                .publish_homeserver(kp, None, PublishStrategy::IfOlderThan)
                .await?;
        } else {
            let client = self.client.clone();
            let kp_cloned = kp.clone();
            let fut = async move {
                let _ = client
                    .publish_homeserver(&kp_cloned, None, PublishStrategy::IfOlderThan)
                    .await;
            };
            #[cfg(not(target_arch = "wasm32"))]
            tokio::spawn(fut);
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(fut);
        }

        // Ensure agent knows its pubky
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(kp.public_key());
        }

        Ok(session)
    }
}
