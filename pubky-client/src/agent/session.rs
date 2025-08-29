use reqwest::{Method, StatusCode};

#[cfg(not(target_arch = "wasm32"))]
use reqwest::{
    RequestBuilder,
    header::{COOKIE, SET_COOKIE},
};

use url::Url;

use pkarr::PublicKey;
use pubky_common::session::Session;

use super::core::PubkyAgent;
use crate::{agent::state::sealed::Sealed, errors::Result, util::check_http_status};

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
