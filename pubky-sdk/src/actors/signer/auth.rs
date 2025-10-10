use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Method;
use url::Url;

use pubky_common::{
    auth::AuthToken,
    crypto::{encrypt, hash},
};

use crate::{
    Capabilities, cross_log,
    errors::{AuthError, Result},
    util::check_http_status,
};

use super::PubkySigner;

impl PubkySigner {
    /// Produces sessions for an app (e.g. Pubky Ring -> App). Sends a signed
    /// `AuthToken` to the relay channel encoded in a `pubkyauth://` URL.
    ///
    /// Typical usage:
    /// - App constructs `PubkyAuthFlow` and subscribe, shows QR/deeplink.
    /// - Signer calls `send_auth_token` with that URL.
    ///
    /// Requirements:
    /// - `pubkyauth:///?caps=…&secret=<b64url>&relay=<relay_base>`
    /// - Channel is derived as `<relay>/<base64url(hash(secret))>`.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the `pubkyauth://` URL is malformed or missing required parameters.
    /// - Returns [`crate::errors::Error::Authentication`] if the secret cannot be decoded or has the wrong length.
    /// - Propagates transport failures when posting to the relay or if the relay responds with a non-success status.
    #[allow(
        clippy::cognitive_complexity,
        reason = "Approving a flow requires a fixed sequence of validation steps kept together for clarity"
    )]
    pub async fn approve_auth(&self, pubkyauth_url: impl AsRef<str>) -> Result<()> {
        let pubkyauth_url = Url::parse(pubkyauth_url.as_ref())?;

        // 1) Extract query params and decode client secret
        let (relay, client_secret) = Self::parse_relay_and_secret(&pubkyauth_url)?;
        cross_log!(info, "Approving auth flow via relay {relay}");

        // 2) Build token with requested capabilities parsed from URL
        let capabilities = Capabilities::from(&pubkyauth_url);
        cross_log!(
            info,
            "Signing capabilities {:?} for auth approval",
            capabilities
        );

        let encrypted_token = self.build_encrypted_token(capabilities, &client_secret);

        // 3) Derive channel: relay/<base64url(hash(secret))>
        let callback_url = Self::derive_callback_url(&relay, &client_secret)?;
        cross_log!(
            info,
            "Posting encrypted auth token to relay channel {}",
            callback_url
        );

        // 4) POST encrypted token
        let response = self
            .client
            .cross_request(Method::POST, callback_url)
            .await?
            .body(encrypted_token)
            .send()
            .await?;

        check_http_status(response).await?;
        cross_log!(info, "Auth token delivered successfully");
        Ok(())
    }

    fn parse_relay_and_secret(pubkyauth_url: &Url) -> Result<(Url, [u8; 32])> {
        let mut relay_param: Option<String> = None;
        let mut secret_param: Option<String> = None;

        for (key, value) in pubkyauth_url.query_pairs() {
            match key.as_ref() {
                "relay" if relay_param.is_none() => relay_param = Some(value.into_owned()),
                "secret" if secret_param.is_none() => secret_param = Some(value.into_owned()),
                _ => {}
            }
        }

        let relay_str = relay_param
            .ok_or_else(|| AuthError::Validation("Missing 'relay' query parameter".to_string()))?;
        let relay = Url::parse(&relay_str)?;

        let secret_str = secret_param
            .ok_or_else(|| AuthError::Validation("Missing 'secret' query parameter".to_string()))?;
        let secret_bytes = URL_SAFE_NO_PAD
            .decode(secret_str)
            .map_err(|e| AuthError::Validation(format!("Invalid base64 secret: {e}")))?;

        let client_secret: [u8; 32] = secret_bytes
            .try_into()
            .map_err(|_err| AuthError::Validation("Client secret must be 32 bytes".to_string()))?;

        Ok((relay, client_secret))
    }

    fn build_encrypted_token(
        &self,
        capabilities: Capabilities,
        client_secret: &[u8; 32],
    ) -> Vec<u8> {
        let token = AuthToken::sign(&self.keypair, capabilities);
        encrypt(&token.serialize(), client_secret)
    }

    fn derive_callback_url(relay: &Url, client_secret: &[u8; 32]) -> Result<Url> {
        let mut callback_url = relay.clone();
        let mut path_segments = callback_url
            .path_segments_mut()
            .map_err(|()| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        path_segments.pop_if_empty();
        let channel_id = URL_SAFE_NO_PAD.encode(hash(client_secret).as_bytes());
        path_segments.push(&channel_id);
        drop(path_segments);
        Ok(callback_url)
    }
}
