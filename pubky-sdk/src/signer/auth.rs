use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{IntoUrl, Method};
use std::collections::HashMap;
use url::Url;

use pubky_common::{
    auth::AuthToken,
    crypto::{encrypt, hash},
};

use crate::{
    Capabilities,
    errors::{AuthError, Result},
    util::check_http_status,
};

use super::PubkySigner;

impl PubkySigner {
    /// Produces sessions for an app (e.g. Pubky Ring -> App). Sends a signed
    /// `AuthToken` to the relay channel encoded in a `pubkyauth://` URL.
    ///
    /// Typical usage:
    /// - App constructs `PubkyAuthRequest` and subscribe, shows QR/deeplink.
    /// - Signer calls `send_auth_token` with that URL.
    ///
    /// Requirements:
    /// - `pubkyauth:///?caps=…&secret=<b64url>&relay=<relay_base>`
    /// - Channel is derived as `<relay>/<base64url(hash(secret))>`.
    pub async fn approve_pubkyauth_request<T: IntoUrl>(&self, pubkyauth_url: &T) -> Result<()> {
        let pubkyauth_url = Url::parse(pubkyauth_url.as_str())?;

        // 1) Extract query params
        let query_params: HashMap<String, String> =
            pubkyauth_url.query_pairs().into_owned().collect();

        let relay_str = query_params
            .get("relay")
            .ok_or_else(|| AuthError::Validation("Missing 'relay' query parameter".to_string()))?;
        let relay = Url::parse(relay_str)?;

        let secret_str = query_params
            .get("secret")
            .ok_or_else(|| AuthError::Validation("Missing 'secret' query parameter".to_string()))?;

        // 2) Decode client secret
        let secret_bytes = URL_SAFE_NO_PAD
            .decode(secret_str)
            .map_err(|e| AuthError::Validation(format!("Invalid base64 secret: {e}")))?;

        let client_secret: [u8; 32] = secret_bytes
            .try_into()
            .map_err(|_| AuthError::Validation("Client secret must be 32 bytes".to_string()))?;

        // 3) Build token with requested capabilities parsed from URL
        let capabilities = Capabilities::from(&pubkyauth_url);

        let token = AuthToken::sign(&self.keypair, capabilities);
        let encrypted_token = encrypt(&token.serialize(), &client_secret);

        // 4) Derive channel: relay/<base64url(hash(secret))>
        let mut callback_url = relay.clone();
        let mut path_segments = callback_url
            .path_segments_mut()
            .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        path_segments.pop_if_empty();
        let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
        path_segments.push(&channel_id);
        drop(path_segments);

        // 5) POST encrypted token
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
