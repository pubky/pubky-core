use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Method;
use std::str::FromStr;
use url::Url;

use pubky_common::{
    auth::AuthToken,
    crypto::{encrypt, hash},
};

use crate::{
    cross_log,
    deep_links::DeepLink,
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
    pub async fn approve_auth(&self, pubkyauth_url: impl AsRef<str>) -> Result<()> {
        let deep_link = DeepLink::from_str(pubkyauth_url.as_ref())
            .map_err(|e| AuthError::Validation(format!("Invalid pubkyauth URL: {e}")))?;

        match deep_link {
            DeepLink::Signup(signup) => {
                if signup.is_direct_signup() {
                    cross_log!(
                        info,
                        "Approving direct signup for homeserver {}",
                        signup.homeserver()
                    );
                    self.signup(signup.homeserver(), signup.signup_token().as_deref())
                        .await?;
                    return Ok(());
                }

                let relay = signup.relay().ok_or_else(|| {
                    AuthError::Validation("Missing 'relay' query parameter".to_string())
                })?;
                let client_secret = signup.secret().ok_or_else(|| {
                    AuthError::Validation("Missing 'secret' query parameter".to_string())
                })?;
                self.post_auth_token(relay, client_secret, signup.capabilities())
                    .await?;
            }
            DeepLink::Signin(signin) => {
                self.post_auth_token(signin.relay(), signin.secret(), signin.capabilities())
                    .await?;
            }
            DeepLink::SeedExport(_) => {
                return Err(AuthError::Validation(
                    "Seed export deep link is not an auth approval request".to_string(),
                )
                .into());
            }
        }
        Ok(())
    }

    fn build_encrypted_token(
        &self,
        capabilities: crate::Capabilities,
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

    async fn post_auth_token(
        &self,
        relay: &Url,
        client_secret: &[u8; 32],
        capabilities: &crate::Capabilities,
    ) -> Result<()> {
        cross_log!(info, "Approving auth flow via relay {relay}");
        cross_log!(
            info,
            "Signing capabilities {:?} for auth approval",
            capabilities
        );

        let encrypted_token = self.build_encrypted_token(capabilities.clone(), client_secret);
        let callback_url = Self::derive_callback_url(relay, client_secret)?;
        cross_log!(
            info,
            "Posting encrypted auth token to relay channel {}",
            callback_url
        );

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
}
