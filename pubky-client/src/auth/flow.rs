use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::{AbortHandle, Abortable};
use reqwest::Method;
use std::sync::Arc;
use url::Url;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

use crate::{
    Capabilities, KeylessAgent, PubkyClient, cross_debug,
    errors::{AuthError, Result},
    global::global_client,
    util::check_http_status,
};
use pubky_common::{
    auth::AuthToken,
    crypto::{decrypt, hash, random_bytes},
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

    /// Start listening to the relay channel in the background and return a subscription handle.
    /// The background task stops when the handle is dropped or after the first result is delivered.
    pub fn subscribe(&self) -> AuthSubscription {
        let (tx, rx) = flume::bounded(1);

        let client = self.client.clone();
        let relay_channel_url = self.relay_channel_url.clone();
        let client_secret = self.client_secret; // copy

        let (abort_handle, abort_reg) = AbortHandle::new_pair();

        // Background polling future (single-shot delivery)
        let fut = async move {
            let res = Self::poll_for_token(client, relay_channel_url, client_secret).await;
            // Ignore send failure if the receiver was dropped.
            let _ = tx.send(res);
        };

        // Spawn abortable task cross-platform
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _join = tokio::spawn(Abortable::new(fut, abort_reg));
            // We don't store JoinHandle: abort on Drop via AbortHandle is enough.
        }
        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(Abortable::new(fut, abort_reg).map(|_| ()));
        }

        AuthSubscription {
            client: self.client.clone(),
            rx,
            abort: abort_handle,
        }
    }

    // Private helper used by subscribe(); mirrors wait_for_response() with status mapping.
    async fn poll_for_token(
        client: Arc<PubkyClient>,
        relay_channel_url: Url,
        client_secret: [u8; 32],
    ) -> Result<AuthToken> {
        use reqwest::StatusCode;

        let response = loop {
            match client
                .cross_request(Method::GET, relay_channel_url.clone())
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

        if response.status() == StatusCode::NOT_FOUND || response.status() == StatusCode::GONE {
            return Err(AuthError::RequestExpired.into());
        }

        let response = check_http_status(response).await?;
        let encrypted = response.bytes().await?;
        let token_bytes = decrypt(&encrypted, &client_secret)?;
        let token = AuthToken::verify(&token_bytes)?;
        Ok(token)
    }
}

/// Handle returned by `AuthFlow::subscribe()`.
#[derive(Debug)]
pub struct AuthSubscription {
    client: Arc<PubkyClient>,
    rx: flume::Receiver<Result<AuthToken>>,
    abort: AbortHandle,
}

impl AuthSubscription {
    /// Await the verified `AuthToken`. Aborts on Drop.
    pub async fn token(self) -> Result<AuthToken> {
        match self.rx.recv_async().await {
            Ok(res) => res,
            Err(_) => Err(AuthError::RequestExpired.into()),
        }
    }

    /// Await and convert into a session-bound keyless agent. Aborts on Drop.
    pub async fn into_agent(self) -> Result<KeylessAgent> {
        let agent = KeylessAgent::with_client(self.client.clone());
        let token = self.token().await?;
        let _session = agent.signin_with_authtoken(&token).await?;
        Ok(agent)
    }

    /// Non-blocking check: returns immediately if a result is ready.
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        self.rx.try_recv().ok()
    }
}

impl Drop for AuthSubscription {
    fn drop(&mut self) {
        // Stop background polling immediately.
        self.abort.abort();
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
