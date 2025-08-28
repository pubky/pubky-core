use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{IntoUrl, Method};
use url::Url;

use pubky_common::{
    auth::AuthToken,
    crypto::{encrypt, hash},
};

use crate::util::check_http_status;
use crate::{
    agent::state::{Keyed, sealed::Sealed},
    errors::{AuthError, Result},
};

use super::core::PubkyAgent;
use crate::{Capabilities, Session};

impl PubkyAgent<Keyed> {
    /// Send a signed AuthToken to a relay channel. Requires keypair.
    pub async fn send_auth_token<T: IntoUrl>(&self, pubkyauth_url: &T) -> Result<()> {
        let kp = self.keypair.get();

        let pubkyauth_url = Url::parse(pubkyauth_url.as_str())?;
        let query_params: HashMap<String, String> =
            pubkyauth_url.query_pairs().into_owned().collect();

        let relay_str = query_params
            .get("relay")
            .ok_or_else(|| AuthError::Validation("Missing 'relay' query parameter".to_string()))?;
        let relay = Url::parse(relay_str)?;

        let secret_str = query_params
            .get("secret")
            .ok_or_else(|| AuthError::Validation("Missing 'secret' query parameter".to_string()))?;

        let secret_bytes = URL_SAFE_NO_PAD
            .decode(secret_str)
            .map_err(|e| AuthError::Validation(format!("Invalid base64 secret: {e}")))?;

        let client_secret: [u8; 32] = secret_bytes
            .try_into()
            .map_err(|_| AuthError::Validation("Client secret must be 32 bytes".to_string()))?;

        let capabilities = Capabilities::from(&pubkyauth_url);

        let token = AuthToken::sign(kp, capabilities);
        let encrypted_token = encrypt(&token.serialize(), &client_secret);

        let mut callback_url = relay.clone();
        let mut path_segments = callback_url
            .path_segments_mut()
            .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        path_segments.pop_if_empty();
        let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
        path_segments.push(&channel_id);
        drop(path_segments);

        let response = self
            .client
            .cross_request(Method::POST, callback_url)
            .await?
            .body(encrypted_token)
            .send()
            .await?;

        check_http_status(response).await?;
        Ok(())
    }
}

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
