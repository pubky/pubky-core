use std::fmt;

use futures_util::future::{AbortHandle, Abortable};

use url::Url;

#[allow(deprecated, reason = "Internal use of deprecated public API")]
use crate::{
    AuthToken, PubkyHttpClient, PubkySession,
    actors::{
        DEFAULT_HTTP_RELAY_INBOX,
        auth::{
            http_relay_inbox_channel::EncryptedHttpRelayInboxChannel,
            http_relay_link_channel::EncryptedHttpRelayLinkChannel,
        },
    },
    cross_log,
    errors::{AuthError, Result},
};

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

/// Internal dispatch between inbox and link channel implementations.
///
/// The variant is chosen automatically based on the relay URL path:
/// - Paths ending with `/link` or `/link/` → [`Link`](Self::Link)
/// - Everything else (including `/inbox`) → [`Inbox`](Self::Inbox)
#[derive(Clone)]
#[allow(deprecated, reason = "Internal use of deprecated public API")]
enum EncryptedAuthChannel {
    Inbox(EncryptedHttpRelayInboxChannel),
    Link(EncryptedHttpRelayLinkChannel),
}

#[allow(deprecated, reason = "Internal use of deprecated public API")]
impl EncryptedAuthChannel {
    /// Poll the underlying channel for a message.
    async fn poll(
        &self,
        client: &PubkyHttpClient,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Inbox(ch) => Ok(ch.poll(client, timeout).await?),
            Self::Link(ch) => Ok(ch.poll(client, timeout).await?),
        }
    }

    /// Acknowledge receipt. Only meaningful for inbox channels (no-op for link).
    /// Errors are propagated for inbox so callers know if the ACK failed.
    async fn ack(&self, client: &PubkyHttpClient) -> Result<()> {
        if let Self::Inbox(ch) = self {
            ch.ack(client).await?;
        }
        Ok(())
    }
}

#[allow(deprecated, reason = "Internal use of deprecated public API")]
impl fmt::Display for EncryptedAuthChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inbox(ch) => write!(f, "{ch}"),
            Self::Link(ch) => write!(f, "{ch}"),
        }
    }
}

#[allow(deprecated, reason = "Internal use of deprecated public API")]
impl fmt::Debug for EncryptedAuthChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inbox(ch) => f.debug_tuple("Inbox").field(ch).finish(),
            Self::Link(ch) => f.debug_tuple("Link").field(ch).finish(),
        }
    }
}

/// Returns `true` if the URL path ends with `/link` or `/link/`.
fn is_link_url(url: &Url) -> bool {
    let path = url.path().trim_end_matches('/');
    path.ends_with("/link")
}

/// **auth subscription** (long polling for a single auth token) you *hold on to*.
///
/// Use it like this:
/// 1. Construct with the builder
///    [`AuthSubscription::builder`] to override relay/client.
/// 2. Complete the flow with [`await_approval`](Self::await_approval) **or**
///    poll with [`try_poll_once`](Self::try_poll_once) / [`try_token`](Self::try_token).
///
/// Background polling **starts immediately** at construction. Dropping this value cancels
/// the background task; the relay channel itself expires server-side after its TTL.
#[derive(Debug)]
pub struct AuthSubscription {
    client: PubkyHttpClient,
    rx: flume::Receiver<Result<AuthToken>>,
    abort: AbortHandle,
}

impl AuthSubscription {
    /// Create a builder for [`AuthSubscription`].
    pub fn builder(secret: [u8; 32]) -> AuthSubscriptionBuilder {
        AuthSubscriptionBuilder::new(secret)
    }

    /// Block until the signer approves and the server issues a session.
    ///
    /// This awaits the background poller’s result, verifies/decrypts the token,
    /// and completes the `/session` exchange to return a ready-to-use [`PubkySession`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures while polling the relay.
    /// - Propagates errors from the internal session exchange if it fails.
    pub async fn await_approval(self) -> Result<PubkySession> {
        let token = self.recv_token().await?;
        PubkySession::new(&token, self.client.clone()).await
    }

    /// Block until the signer approves and we receive an [`AuthToken`].
    ///
    /// This awaits the background poller’s result.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures encountered while polling the relay.
    pub async fn await_token(self) -> Result<AuthToken> {
        self.recv_token().await
    }

    async fn recv_token(&self) -> Result<AuthToken> {
        self.rx
            .recv_async()
            .await
            .map_err(|_err| AuthError::RequestExpired)?
    }

    /// Non-blocking probe (single step) that **consumes any ready token** and returns:
    /// - `Ok(Some(session))` when a token was delivered and the session established.
    /// - `Ok(None)` if no payload yet (keep polling later).
    /// - `Err(e)` on transport/server errors or if the channel expired.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expired before a token arrived.
    /// - Propagates HTTP/transport failures from constructing the session.
    pub async fn try_poll_once(&self) -> Result<Option<PubkySession>> {
        if let Some(tok) = self.try_token() {
            let token = tok?;
            return Ok(Some(PubkySession::new(&token, self.client.clone()).await?));
        }
        Ok(None)
    }

    /// Non-blocking check: returns a verified `AuthToken` if the background poller has delivered it.
    ///
    /// - `Some(Ok(AuthToken))` when ready.
    /// - `Some(Err(_))` if the background task failed (expired/transport error).
    /// - `None` if not yet delivered.
    #[must_use]
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        self.rx.try_recv().ok()
    }

    // -- internals --

    /// Long-poll until a token arrives or the channel expires. Runs in the background task.
    async fn poll_for_token_loop(
        client: PubkyHttpClient,
        encrypted_channel: EncryptedAuthChannel,
        tx: flume::Sender<Result<AuthToken>>,
    ) {
        cross_log!(
            info,
            "Starting auth flow polling for relay channel {}",
            encrypted_channel
        );
        let result = Self::poll_for_token(&client, &encrypted_channel).await;

        if result.is_ok() {
            cross_log!(
                info,
                "Auth flow successfully decrypted token for relay channel {}",
                encrypted_channel
            );
        }

        let _ = tx.send(result);
    }

    async fn poll_for_token(
        client: &PubkyHttpClient,
        encrypted_channel: &EncryptedAuthChannel,
    ) -> Result<AuthToken> {
        let response = encrypted_channel
            .poll(client, None)
            .await?
            .ok_or(AuthError::RequestExpired)?;
        let token = AuthToken::verify(&response)?;

        // ACK: confirms receipt for inbox channels, no-op for link.
        // Best-effort: a failed ACK should not invalidate a verified token.
        if let Err(e) = encrypted_channel.ack(client).await {
            cross_log!(warn, "Inbox ACK failed (non-fatal): {e}");
        }

        Ok(token)
    }
}

impl Drop for AuthSubscription {
    fn drop(&mut self) {
        // Stop background polling immediately.
        self.abort.abort();
    }
}

/// Builder for [`PubkyAuthFlow`].
///
/// Use to override the HTTP relay and/or the `PubkyHttpClient`.
#[derive(Debug, Clone)]
pub struct AuthSubscriptionBuilder {
    relay_base_url: Url,
    secret: [u8; 32],
    client: Option<PubkyHttpClient>,
}

#[allow(deprecated, reason = "Internal use of deprecated public API")]
impl AuthSubscriptionBuilder {
    pub(crate) fn new(secret: [u8; 32]) -> Self {
        Self {
            relay_base_url: Url::parse(DEFAULT_HTTP_RELAY_INBOX).expect("Always valid"),
            secret,
            client: None,
        }
    }

    /// Set a custom relay base URL. Trailing slash optional.
    pub fn relay_base_url(mut self, url: Url) -> Self {
        self.relay_base_url = url;
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    // Spawn background polling (single-shot delivery)
    fn spawn_background_polling(
        encrypted_channel: EncryptedAuthChannel,
        client: PubkyHttpClient,
    ) -> AuthSubscription {
        let (tx, rx) = flume::bounded(1);
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        let bg_client = client.clone();

        let fut = async move {
            cross_log!(info, "Spawning auth flow polling task");
            AuthSubscription::poll_for_token_loop(bg_client, encrypted_channel.clone(), tx).await;
        };

        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(Abortable::new(fut, abort_reg));

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(Abortable::new(fut, abort_reg).map(|_| ()));

        AuthSubscription {
            client,
            rx,
            abort: abort_handle,
        }
    }

    /// Finalize: derive channel, spawn the background poller,
    /// and return the subscription handle.
    ///
    /// The channel type is auto-detected from the relay URL path:
    /// - `/link` or `/link/` → link channel (synchronous pairing)
    /// - Otherwise → inbox channel (store-and-forward, default)
    pub fn start(self) -> Result<AuthSubscription> {
        let client = match self.client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        let encrypted_channel = if is_link_url(&self.relay_base_url) {
            EncryptedAuthChannel::Link(EncryptedHttpRelayLinkChannel::new(
                self.relay_base_url,
                self.secret,
            )?)
        } else {
            EncryptedAuthChannel::Inbox(EncryptedHttpRelayInboxChannel::new(
                self.relay_base_url,
                self.secret,
            )?)
        };

        Ok(Self::spawn_background_polling(encrypted_channel, client))
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::capabilities::Capabilities;

    use super::*;
    use crate::Keypair;

    #[test]
    fn is_link_url_with_link_path() {
        let url = Url::parse("https://relay.example.com/link").unwrap();
        assert!(is_link_url(&url));
    }

    #[test]
    fn is_link_url_with_link_trailing_slash() {
        let url = Url::parse("https://relay.example.com/link/").unwrap();
        assert!(is_link_url(&url));
    }

    #[test]
    fn is_link_url_with_inbox_path() {
        let url = Url::parse("https://relay.example.com/inbox").unwrap();
        assert!(!is_link_url(&url));
    }

    #[test]
    fn is_link_url_with_nested_link_path() {
        let url = Url::parse("https://relay.example.com/api/v1/link").unwrap();
        assert!(is_link_url(&url));
    }

    #[test]
    fn is_link_url_with_root_path() {
        let url = Url::parse("https://relay.example.com/").unwrap();
        assert!(!is_link_url(&url));
    }

    #[test]
    fn is_link_url_with_link_prefix_not_suffix() {
        // "linkage" ends with "linkage", not "/link"
        let url = Url::parse("https://relay.example.com/linkage").unwrap();
        assert!(!is_link_url(&url));
    }

    #[tokio::test]
    async fn subscribe_to_auth_token() {
        // Start a local relay so the test doesn't depend on production.
        let relay = http_relay::HttpRelay::builder()
            .http_port(0)
            .run()
            .await
            .unwrap();
        let inbox_base = relay.local_url().join("inbox").unwrap();

        let encrypted_channel =
            EncryptedHttpRelayInboxChannel::random_secret(inbox_base.clone()).unwrap();

        let keypair = Keypair::random();
        let capabilities = Capabilities::default();
        let token = AuthToken::sign(&keypair, capabilities);
        let token_bytes = token.serialize();

        let main_client = PubkyHttpClient::new().unwrap();

        let client = main_client.clone();
        let channel = encrypted_channel.clone();
        let producer_handle = tokio::spawn(async move {
            channel.produce(&client, &token_bytes).await.unwrap();
        });

        let subscriber = AuthSubscription::builder(encrypted_channel.secret().to_owned())
            .relay_base_url(inbox_base)
            .client(main_client.clone())
            .start()
            .unwrap();
        let poll_handle = tokio::spawn(async move {
            let response = subscriber.await_token().await.unwrap();
            assert_eq!(response, token);
        });

        let (producer_result, poll_result) = tokio::join!(producer_handle, poll_handle);
        producer_result.unwrap();
        poll_result.unwrap();
    }
}
