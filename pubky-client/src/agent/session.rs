use reqwest::{Method, StatusCode};

#[cfg(not(target_arch = "wasm32"))]
use reqwest::{
    RequestBuilder,
    header::{COOKIE, SET_COOKIE},
};

use url::Url;

use pkarr::PublicKey;
use pubky_common::{auth::AuthToken, capabilities::Capability, session::Session};

use crate::{
    PubkyAgent,
    agent::state::{Keyed, sealed::Sealed},
    errors::Result,
    util::check_http_status,
};

impl<S: Sealed> PubkyAgent<S> {
    /// Retrieve session for current pubky. Fails if pubky is unknown.
    pub async fn session(&self) -> Result<Option<Session>> {
        let response = self
            .homeserver()
            .request(Method::GET, "/session")
            .await?
            .send()
            .await?;
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
    pub(crate) fn url_is_this_agents_homeserver(&self, url: &Url) -> bool {
        let Some(agent_pk) = self.pubky() else {
            return false;
        };
        let host = url.host_str().unwrap_or("");
        // match either "_pubky.<pk>" or "<pk>"
        if let Some(tail) = host.strip_prefix("_pubky.") {
            PublicKey::try_from(tail)
                .ok()
                .map_or(false, |h| h == agent_pk)
        } else {
            PublicKey::try_from(host)
                .ok()
                .map_or(false, |h| h == agent_pk)
        }
    }

    /// We only capture session cookie on our two trusted auth flows.
    ///
    /// * `signup()` => `POST https://<homeserver>/signup`
    /// * `signin_with_authtoken()` => `POST pubky://<user>/session`
    ///
    /// Do not use on requests to third party servers.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_session_cookie(&self, response: &reqwest::Response) {
        // 1) If we don’t know our pubky yet, we can’t determine the cookie name, bail.
        let Ok(pk) = self.require_pubky() else { return };
        let cookie_name = pk.to_string();

        // 2) Scan Set-Cookie headers; store the one that matches our `<pubky>=<secret>` cookie.
        for val in response.headers().get_all(SET_COOKIE).iter() {
            let Ok(raw) = std::str::from_utf8(val.as_bytes()) else {
                continue;
            };
            let Ok(parsed) = cookie::Cookie::parse(raw.to_owned()) else {
                continue;
            };

            if parsed.name() == cookie_name {
                if let Ok(mut slot) = self.session_secret.write() {
                    *slot = Some(parsed.value().to_string());
                }
                break; // we found ours; stop scanning
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn maybe_attach_session_cookie(
        &self,
        url: &Url,
        rb: RequestBuilder,
    ) -> Result<RequestBuilder> {
        // 1) Only attach cookies when the target is *this* agent’s homeserver.
        if !self.url_is_this_agents_homeserver(url) {
            return Ok(rb);
        }

        // 2) Read the per-agent session secret (if any). If the lock is poisoned or empty, skip.
        let guard = match self.session_secret.read() {
            Ok(g) => g,
            Err(_) => return Ok(rb),
        };
        let secret = match guard.as_ref() {
            Some(s) => s,
            None => return Ok(rb),
        };

        // 3) Compute the cookie name from the agent’s pubky and attach the header.
        //    (require_pubky() should succeed here because url matched this agent.)
        let cookie_name = self.require_pubky()?.to_string();
        Ok(rb.header(COOKIE, format!("{cookie_name}={secret}")))
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

        self.pkdns()
            .publish_homeserver_force(Some(&homeserver))
            .await?;

        // On successful signup, the agent’s pubky is known from the keypair.
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(kp.public_key());
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&response);

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
        }

        // Ensure agent knows its pubky
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(kp.public_key());
        }

        Ok(session)
    }
}
