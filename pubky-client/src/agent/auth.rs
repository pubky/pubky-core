use reqwest::Method;

use pubky_common::auth::AuthToken;

use crate::util::check_http_status;
use crate::{agent::state::sealed::Sealed, errors::Result};

use super::core::PubkyAgent;
use crate::Session;

// Shared internals used by both sides (Keyed and Keyless)
impl<S: Sealed> PubkyAgent<S> {
    pub async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
        let url = format!("pubky://{}/session", token.pubky());
        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;

        // Set pubky and capture cookie for this identity,
        // must be set before we try to capture the session cookie.
        self.set_pubky_if_empty(token.pubky());
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&response);

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }
}
