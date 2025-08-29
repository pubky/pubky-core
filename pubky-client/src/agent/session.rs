use reqwest::{Method, StatusCode};
use url::Url;

#[cfg(not(target_arch = "wasm32"))]
use reqwest::{
    RequestBuilder,
    header::{COOKIE, SET_COOKIE},
};

use pkarr::PublicKey;
use pubky_common::session::Session;

use super::core::PubkyAgent;
use crate::{errors::Result, util::check_http_status};

impl PubkyAgent {
    /// Retrieve session for current pubky. Fails if pubky is unknown.
    pub async fn session(&self) -> Result<Option<Session>> {
        let response = self
            .drive()
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

    /// Sign out and invalidate this agent’s server-side session. Consumes the agent.
    pub async fn signout(self) -> Result<()> {
        let response = self.drive().delete("/session").await?;
        check_http_status(response).await?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn url_is_this_agents_homeserver(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or("");
        let pubky = self.session.pubky();
        if let Some(tail) = host.strip_prefix("_pubky.") {
            PublicKey::try_from(tail)
                .ok()
                .map_or(false, |h| &h == pubky)
        } else {
            PublicKey::try_from(host)
                .ok()
                .map_or(false, |h| &h == pubky)
        }
    }

    /// Extract `<pubky>=<secret>` from `Set-Cookie` on trusted auth flows.
    /// Uses `pubky` as to look for the cookie.
    ///
    /// Trusted entrypoints:
    /// - `POST pubky://<homeserver>/signup`
    /// - `POST pubky://<user>/session`
    ///
    /// Returns `Some(secret)` if found, otherwise `None`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn capture_session_cookie(
        pubky: &PublicKey,
        response: &reqwest::Response,
    ) -> Option<String> {
        // 1) The cookie is named as our agent's pubky.
        let cookie_name = pubky.to_string();

        // 2) Scan Set-Cookie headers; store the one that matches our `<pubky>=<secret>` cookie.
        for val in response.headers().get_all(SET_COOKIE).iter() {
            let Ok(raw) = std::str::from_utf8(val.as_bytes()) else {
                continue;
            };
            let Ok(parsed) = cookie::Cookie::parse(raw.to_owned()) else {
                continue;
            };
            if parsed.name() == cookie_name {
                return Some(parsed.value().to_string());
            }
        }
        None
    }

    /// Attach session cookie only for this agent’s homeserver (native only).
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

        // 2) Compute the cookie name from the agent’s pubky and attach the cookie.
        Ok(rb.header(COOKIE, format!("{}={}", self.pubky(), self.cookie)))
    }
}
