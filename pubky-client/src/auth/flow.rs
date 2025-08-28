use std::sync::Arc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Method;
use url::Url;

use pubky_common::{
    auth::AuthToken,
    crypto::{decrypt, hash, random_bytes},
};

use crate::{
    Capabilities, KeylessAgent, PubkyClient, cross_debug, errors::Result, global::global_client,
};

/// Default relay when none is supplied.
pub const DEFAULT_HTTP_RELAY: &str = "https://httprelay.pubky.app/link/";

/// Orchestrates the pubkyauth handshake for keyless apps.
/// Build once, display its `pubkyauth://` URL (QR/deep-link), then block on `wait_for_response`
/// or jump straight to `into_agent` to establish a session.
#[derive(Debug, Clone)]
pub struct AuthFlow {
    client: Arc<PubkyClient>,
    client_secret: [u8; 32],
    pubkyauth_url: Url,
    relay_channel_url: Url,
}

impl AuthFlow {
    /// Construct an Auth flow on a specific `PubkyClient`.
    ///
    /// - `relay`: optional base relay URL. The channel path segment is derived internally
    ///   as `base64url(hash(client_secret))` and appended to the relay URL. If `None`,
    ///   `DEFAULT_HTTP_RELAY` is used.
    /// - `caps`: capabilities requested for the response token.
    pub fn new_with_client(
        client: Arc<PubkyClient>,
        relay: Option<impl Into<Url>>,
        caps: &Capabilities,
    ) -> Result<Self> {
        // Resolve relay base
        let mut relay_url = match relay {
            Some(r) => r.into(),
            None => Url::parse(DEFAULT_HTTP_RELAY)?,
        };

        // 1) Client secret and user-displayable pubkyauth URL
        let client_secret = random_bytes::<32>();
        let pubkyauth_url = Url::parse(&format!(
            "pubkyauth:///?caps={caps}&secret={}&relay={relay_url}",
            URL_SAFE_NO_PAD.encode(client_secret)
        ))?;

        // 2) Derive the relay channel URL from the client secret hash
        let mut segments = relay_url
            .path_segments_mut()
            .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        segments.pop_if_empty();
        let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
        segments.push(&channel_id);
        drop(segments);

        Ok(Self {
            client,
            client_secret,
            pubkyauth_url,
            relay_channel_url: relay_url,
        })
    }

    /// Lazily constructs using the process-wide shared `PubkyClient`.
    pub fn new(relay: Option<impl Into<Url>>, caps: &Capabilities) -> Result<Self> {
        let client = global_client()?;
        Self::new_with_client(client, relay, caps)
    }

    /// The deep-link or QR to present to the user on the authenticating device.
    #[inline]
    pub fn pubkyauth_url(&self) -> &Url {
        &self.pubkyauth_url
    }

    /// Block until the PubkySigner posts an encrypted token to the relay channel.
    ///
    /// Behavior:
    /// - Performs a GET on the derived channel URL.
    /// - Retries on timeouts. Propagates non-timeout transport errors.
    /// - Decrypts with the client secret and verifies the token signature.
    pub async fn wait_for_response(&self) -> Result<AuthToken> {
        let response = loop {
            match self
                .client
                .cross_request(Method::GET, self.relay_channel_url.clone())
                .await?
                .send()
                .await
            {
                Ok(resp) => break resp,
                Err(e) => {
                    if e.is_timeout() {
                        cross_debug!("HttpRelay timed out; retrying channel poll …");
                        continue;
                    }
                    return Err(e.into());
                }
            }
        };

        let encrypted = response.bytes().await?;
        let token_bytes = decrypt(&encrypted, &self.client_secret)?;
        let token = AuthToken::verify(&token_bytes)?;
        Ok(token)
    }

    /// Convenience: consume the flow and establish a session-bound keyless agent.
    ///
    /// Steps:
    /// 1) Wait for the auth token.
    /// 2) POST `pubky://<user>/session` with the verified token bytes.
    /// 3) Capture the session cookie (native) and set the agent’s pubky.
    pub async fn into_agent(self) -> Result<KeylessAgent> {
        let token = self.wait_for_response().await?;
        self.into_agent_with_token(token).await
    }

    /// Same as `into_agent`, but caller provides the already-verified token.
    pub async fn into_agent_with_token(self, token: AuthToken) -> Result<KeylessAgent> {
        // Build a keyless agent on the same client and reuse existing internal signin helper.
        let agent = KeylessAgent::with_client(self.client);

        // This calls the homeserver, captures cookie, sets agent.pubky(), and returns Session.
        let _session = agent.signin_with_authtoken(&token).await?;

        Ok(agent)
    }
}

#[cfg(test)]
impl AuthFlow {
    /// Returns the derived relay channel URL, mainly for diagnostics.
    #[cfg(test)]
    pub fn relay_channel_url(&self) -> &Url {
        &self.relay_channel_url
    }

    /// Decode utilities for testing.
    #[inline]
    pub fn client_secret(&self) -> &[u8; 32] {
        &self.client_secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_urls_and_channel() {
        let caps = Capabilities::default();
        let relay = Url::parse("https://http-relay.example.com/link/").unwrap();

        let flow = AuthFlow::new(Some(relay.clone()), &caps).unwrap();
        assert!(
            flow.pubkyauth_url()
                .as_str()
                .starts_with("pubkyauth:///?caps=")
        );
        assert!(
            flow.pubkyauth_url()
                .query_pairs()
                .any(|(k, _)| k == "secret")
        );
        assert!(
            flow.relay_channel_url()
                .as_str()
                .starts_with("https://http-relay.example.com/")
        );
        // Channel id must be last segment derived from client_secret hash
        let last_seg = flow
            .relay_channel_url()
            .path_segments()
            .and_then(|mut it| it.next_back())
            .unwrap();
        assert_eq!(
            last_seg,
            URL_SAFE_NO_PAD.encode(hash(flow.client_secret()).as_bytes())
        );
    }
}
