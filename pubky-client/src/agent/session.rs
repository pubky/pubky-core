use reqwest::{Method, StatusCode};

#[cfg(not(target_arch = "wasm32"))]
use reqwest::header::SET_COOKIE;

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
}
