use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Method;
use url::Url;

use pubky_common::{
    auth::{
        AuthToken,
        grant::GrantClaims,
        jws::{ClientId, GrantId},
    },
    crypto::{PublicKey, encrypt, hash},
};

use crate::{
    Capabilities, cross_log,
    actors::auth::deep_links::{DeepLink, DeepLinkParseError},
    errors::{AuthError, Result},
    util::check_http_status,
};

use super::PubkySigner;

/// Default lifetime for issued grants: 2 years (per proposal v4-pop §"Token Lifetime").
const DEFAULT_GRANT_LIFETIME_SECS: u64 = 2 * 365 * 24 * 3600;

impl PubkySigner {
    /// Produces sessions for an app (e.g. Pubky Ring -> App). Sends a signed
    /// `AuthToken` (legacy flow) or a signed `pubky-grant` JWS (grant + JWT
    /// flow) to the relay channel encoded in a `pubkyauth://` URL.
    ///
    /// Typical usage:
    /// - App constructs `PubkyAuthFlow` and subscribe, shows QR/deeplink.
    /// - Signer calls `send_auth_token` with that URL.
    ///
    /// Requirements:
    /// - URL parses as a [`DeepLink::Signin`], [`DeepLink::Signup`],
    ///   [`DeepLink::SigninJwt`], or [`DeepLink::SignupJwt`].
    /// - Channel is derived as `<relay>/<base64url(hash(secret))>`.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the `pubkyauth://`
    ///   URL is malformed or addresses an intent that `approve_auth` does not
    ///   handle (e.g. `secret_export`).
    /// - Propagates transport failures when posting to the relay or if the
    ///   relay responds with a non-success status.
    pub async fn approve_auth(&self, pubkyauth_url: impl AsRef<str>) -> Result<()> {
        let deep_link: DeepLink = pubkyauth_url
            .as_ref()
            .parse()
            .map_err(|e: DeepLinkParseError| {
                AuthError::Validation(format!("invalid pubkyauth URL: {e}"))
            })?;

        let (relay, client_secret, encrypted_payload) = match &deep_link {
            DeepLink::Signin(d) => {
                cross_log!(
                    info,
                    "Approving legacy signin via relay {} (caps={:?})",
                    d.relay(),
                    d.capabilities()
                );
                let payload = self.build_encrypted_token(d.capabilities().clone(), d.secret());
                (d.relay().clone(), *d.secret(), payload)
            }
            DeepLink::Signup(d) => {
                cross_log!(
                    info,
                    "Approving legacy signup via relay {} (caps={:?})",
                    d.relay(),
                    d.capabilities()
                );
                let payload = self.build_encrypted_token(d.capabilities().clone(), d.secret());
                (d.relay().clone(), *d.secret(), payload)
            }
            DeepLink::SigninJwt(d) => {
                cross_log!(
                    info,
                    "Approving JWT signin via relay {} (client_id={}, caps={:?})",
                    d.relay(),
                    d.client_id(),
                    d.capabilities()
                );
                let payload = self.build_encrypted_grant(
                    d.capabilities().clone(),
                    d.client_id().clone(),
                    d.client_pk().clone(),
                    d.secret(),
                );
                (d.relay().clone(), *d.secret(), payload)
            }
            DeepLink::SignupJwt(d) => {
                cross_log!(
                    info,
                    "Approving JWT signup via relay {} (client_id={}, caps={:?})",
                    d.relay(),
                    d.client_id(),
                    d.capabilities()
                );
                let payload = self.build_encrypted_grant(
                    d.capabilities().clone(),
                    d.client_id().clone(),
                    d.client_pk().clone(),
                    d.secret(),
                );
                (d.relay().clone(), *d.secret(), payload)
            }
            DeepLink::SeedExport(_) => {
                return Err(AuthError::Validation(
                    "approve_auth does not handle seed_export deep links".into(),
                )
                .into());
            }
        };

        let callback_url = Self::derive_callback_url(&relay, &client_secret)?;
        cross_log!(
            info,
            "Posting encrypted auth payload to relay channel {}",
            callback_url
        );

        let response = self
            .client
            .cross_request(Method::POST, callback_url)
            .await?
            .body(encrypted_payload)
            .send()
            .await?;

        check_http_status(response).await?;
        cross_log!(info, "Auth payload delivered successfully");
        Ok(())
    }

    fn build_encrypted_grant(
        &self,
        capabilities: Capabilities,
        client_id: ClientId,
        client_pk: PublicKey,
        client_secret: &[u8; 32],
    ) -> Vec<u8> {
        let now = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let claims = GrantClaims {
            iss: self.keypair.public_key(),
            client_id,
            caps: capabilities.0,
            cnf: client_pk,
            jti: GrantId::generate(),
            iat: now,
            exp: now + DEFAULT_GRANT_LIFETIME_SECS,
        };
        let grant_jws = pubky_common::auth::jws::sign_jws(&self.keypair, "pubky-grant", &claims);
        encrypt(grant_jws.as_bytes(), client_secret)
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
