use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::{AbortHandle, Abortable};
use reqwest::Method;
use std::sync::Arc;
use url::Url;

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

use crate::{
    Capabilities, PubkyAgent, PubkyClient, cross_debug,
    errors::{AuthError, Result},
    global::global_client,
    util::check_http_status,
};
use pubky_common::{
    auth::AuthToken,
    crypto::{decrypt, hash, random_bytes},
};

/// Default HTTP relay when none is supplied.
///
/// The per-flow channel segment is appended automatically as:
/// `base + base64url(hash(client_secret))`.
/// A trailing slash on `base` is optional.
pub const DEFAULT_HTTP_RELAY: &str = "https://httprelay.pubky.app/link/";

/// Pubkyauth handshake for keyless apps.
///
/// One `PubkyAuth` <=> one relay channel (single-use).
///
/// Typical usage:
/// 1. Create with [`PubkyAuth::new`].
/// 2. Call [`PubkyAuth::subscribe`] to start background polling and obtain the `pubkyauth://` URL.
/// 3. Show the returned URL (QR/deeplink) to the signing device (e.g., Pubky Ring).
/// 4. Await [`AuthSubscription::into_agent`] to obtain a session-bound [`PubkyAgent`].
///
/// Threading:
/// - `PubkyAuth` is cheap to construct; polling runs in a single abortable task spawned by `subscribe`.
#[derive(Debug)]
pub struct PubkyAuth {
    client: Arc<PubkyClient>,
    client_secret: [u8; 32],
    pubkyauth_url: Url,
    relay_channel_url: Url,
}

impl PubkyAuth {
    /// Build an auth flow bound to a specific `PubkyClient`.
    ///
    /// Relay:
    /// - If `relay` is `Some`, use it as the base URL (trailing slash optional).
    /// - If `None`, use [`DEFAULT_HTTP_RELAY`].
    /// - The channel path segment is derived as `base64url(hash(client_secret))`.
    ///
    /// Capabilities:
    /// - `caps` are encoded into the `pubkyauth://` URL consumed by the signer.
    ///
    /// Errors:
    /// - Returns URL parsing errors for an invalid `relay`.
    pub fn new_with_client(
        client: Arc<PubkyClient>,
        relay: Option<impl Into<Url>>,
        caps: &Capabilities,
    ) -> Result<Self> {
        // 1) Resolve relay base
        let mut relay_url = match relay {
            Some(r) => r.into(),
            None => Url::parse(DEFAULT_HTTP_RELAY)?,
        };

        // 2) Generate client secret and user-displayable pubkyauth:// URL.
        let client_secret = random_bytes::<32>();
        let pubkyauth_url = Url::parse(&format!(
            "pubkyauth:///?caps={caps}&secret={}&relay={relay_url}",
            URL_SAFE_NO_PAD.encode(client_secret)
        ))?;

        // 3) Derive the relay channel URL from the client secret hash
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

    /// Build bound to a default process-wide shared `PubkyClient`.
    /// This is what you want to use for most of your apps.
    ///
    /// Delegates to [`PubkyAuth::new_with_client`].
    ///
    /// Relay:
    /// - If `relay` is `Some`, use it as the base URL (trailing slash optional).
    /// - If `None`, use [`DEFAULT_HTTP_RELAY`].
    /// - The channel path segment is derived as `base64url(hash(client_secret))`.
    ///
    /// Capabilities:
    /// - `caps` are encoded into the `pubkyauth://` URL consumed by the signer.
    ///
    /// Errors:
    /// - Propagates client build failures as `Error::Build`.
    /// - Returns URL parsing errors for an invalid `relay`.
    pub fn new(relay: Option<impl Into<Url>>, caps: &Capabilities) -> Result<Self> {
        let client = global_client()?;
        Self::new_with_client(client, relay, caps)
    }

    /// Consume the PubkyAuth, start background polling, and return `(subscription, pubkyauth_url)`.
    ///
    /// Semantics:
    /// - Single-shot: delivers at most one token.
    /// - Abortable: dropping the subscription cancels polling immediately; pending `token()`/`into_agent()` resolve with `AuthError::RequestExpired`.
    /// - Transport: timeouts are retried in a simple loop; other transport errors propagate.
    ///
    /// Example:
    /// ```no_run
    /// # use pubky::{PubkyAuth, Capabilities};
    /// let caps = Capabilities::default();
    /// let auth = PubkyAuth::new(None, &caps)?;
    /// let (sub, url) = auth.subscribe();
    /// // display `url` as QR / deeplink to the signer
    /// let agent = sub.into_agent().await?;
    /// # Ok::<(), pubky::Error>(())
    /// ```
    pub fn subscribe(self) -> (AuthSubscription, Url) {
        let (tx, rx) = flume::bounded(1);

        let client = self.client.clone();
        let relay_channel_url = self.relay_channel_url.clone();
        let client_secret = self.client_secret;

        let (abort_handle, abort_reg) = AbortHandle::new_pair();

        // Background polling future (single-shot delivery)
        let fut = async move {
            let res = Self::poll_for_token(client, relay_channel_url, client_secret).await;
            // Ignore send failure if the receiver was dropped.
            let _ = tx.send(res);
        };

        // Spawn abortable task cross-platform
        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(Abortable::new(fut, abort_reg));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(Abortable::new(fut, abort_reg).map(|_| ()));

        let auth_subscription = AuthSubscription {
            client: self.client,
            rx,
            abort: abort_handle,
        };
        (auth_subscription, self.pubkyauth_url)
    }

    /// Poll the relay once a background task is running; decrypt and verify the token.
    ///
    /// Status mapping:
    /// - `404`/`410` => [`AuthError::RequestExpired`].
    /// - Non-2xx => mapped via [`check_http_status`].
    /// - Transport timeout => retry loop; other transport errors propagate.
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
                Ok(r) => break r,
                Err(e) if e.is_timeout() => {
                    cross_debug!("HttpRelay timed out; retrying channel poll …");
                    continue;
                }
                Err(e) => return Err(e.into()),
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

/// Handle returned by [`PubkyAuth::subscribe`].
///
/// Owns the background poll task; delivers exactly one `AuthToken` (or an error).
#[derive(Debug)]
#[must_use = "hold on to this and call token().await or into_agent().await to complete the auth flow"]
pub struct AuthSubscription {
    client: Arc<PubkyClient>,
    rx: flume::Receiver<Result<AuthToken>>,
    abort: AbortHandle,
}

impl AuthSubscription {
    /// Await the verified `AuthToken`.
    ///
    /// Returns:
    /// - `Ok(AuthToken)` on success.
    /// - `Err(AuthError::RequestExpired)` if the relay expired or the subscription was dropped.
    /// - Transport/server errors as appropriate.
    pub async fn token(self) -> Result<AuthToken> {
        match self.rx.recv_async().await {
            Ok(res) => res,
            Err(_) => Err(AuthError::RequestExpired.into()),
        }
    }

    /// Await the token and sign in to obtain a session-bound [`PubkyAgent`].
    ///
    /// Steps:
    /// - Wait for `AuthToken` via [`AuthSubscription::token`].
    /// - POST `pubky://<user>/session` with the token; capture cookie (native) and set pubky.
    ///
    /// Example:
    /// ```no_run
    /// # use pubky::{PubkyAuth, Capabilities};
    /// let (sub, url) = PubkyAuth::new(None, &Capabilities::default())?.subscribe();
    /// // display `url` to signer ...
    /// let agent = sub.into_agent().await?;
    /// # Ok::<(), pubky::Error>(())
    /// ```
    pub async fn into_agent(self) -> Result<PubkyAgent> {
        PubkyAgent::new(self.client.clone(), &self.token().await?).await
    }

    /// Non-blocking probe for readiness.
    ///
    /// Returns:
    /// - `Some(Ok(AuthToken))` if already received.
    /// - `Some(Err(_))` if polling failed.
    /// - `None` if not ready yet.
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
impl PubkyAuth {
    /// Returns the derived relay channel URL, mainly for diagnostics.
    pub fn relay_channel_url(&self) -> &Url {
        &self.relay_channel_url
    }

    /// Return the `pubkyauth://` deep link to show as QR or open via deeplink.
    #[inline]
    pub fn pubkyauth_url(&self) -> &Url {
        &self.pubkyauth_url
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

        let auth = PubkyAuth::new(Some(relay.clone()), &caps).unwrap();
        assert!(
            auth.pubkyauth_url()
                .as_str()
                .starts_with("pubkyauth:///?caps=")
        );
        assert!(
            auth.pubkyauth_url()
                .query_pairs()
                .any(|(k, _)| k == "secret")
        );
        assert!(
            auth.relay_channel_url()
                .as_str()
                .starts_with("https://http-relay.example.com/")
        );
        // Channel id must be last segment derived from client_secret hash
        let last_seg = auth
            .relay_channel_url()
            .path_segments()
            .and_then(|mut it| it.next_back())
            .unwrap();
        assert_eq!(
            last_seg,
            URL_SAFE_NO_PAD.encode(hash(auth.client_secret()).as_bytes())
        );
    }
}
