use reqwest::{Method, StatusCode};

use pubky_common::session::SessionInfo;

use crate::actors::storage::resource::resolve_pubky;
use crate::{
    AuthToken, Error, PubkyHttpClient, Result, SessionStorage, cross_log, util::check_http_status,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::errors::AuthError;

/// Stateful, per-identity API driver built on a shared [`PubkyHttpClient`].
///
/// An `PubkySession` represents one user/identity. It optionally holds a `Keypair` (for
/// self-signed flows like `signin()`/`signup()`), and always tracks the user’s `pubky`
/// (either from the keypair or learned later via the pubkyauth flow). On native targets,
/// each agent also owns exactly one session cookie secret; cookies never leak across agents.
///
/// What it does:
/// - Attaches the correct session cookie to requests that target this agent’s homeserver
///   (`https://_pubky.<pubky>/...`), and to nothing else.
/// - Exposes homeserver verbs (`get/put/post/patch/delete/head`) scoped to this identity.
/// - Implements identity flows: `signup`, `signin`, `signout`, `session`, and pubkyauth.
///
/// When to use:
/// - Use `PubkySession` whenever you’re acting “as a user” against a Pubky homeserver.
/// - Use `PubkyHttpClient` only for raw transport or unauthenticated/public operations.
///
/// Concurrency:
/// - `PubkySession` is cheap to clone and thread-safe; it shares the underlying `PubkyHttpClient`.
#[derive(Clone)]
pub struct PubkySession {
    pub(crate) client: PubkyHttpClient,

    /// Known session for this session.
    pub(crate) info: SessionInfo,

    /// Native-only, single session cookie for `_pubky.<pubky>`. Never shared across agents.
    /// Stored as (`cookie_name`, `cookie_value`) where name is UUID and value is session secret.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cookie: (String, String),
}

impl PubkySession {
    /// Establish a session from a signed [`AuthToken`].
    ///
    /// This POSTs the resolved homeserver session endpoint with the token, validates the response
    /// and constructs a new session-bound [`PubkySession`]
    pub(crate) async fn new(token: &AuthToken, client: PubkyHttpClient) -> Result<Self> {
        let url = format!("pubky://{}/session", token.public_key());
        cross_log!(
            info,
            "Establishing new session exchange for {}",
            token.public_key()
        );
        let resolved = resolve_pubky(&url)?;
        let response = client
            .cross_request(Method::POST, resolved)
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;
        cross_log!(
            info,
            "Session exchange for {} succeeded; constructing session",
            token.public_key()
        );
        Self::new_from_response(client.clone(), response).await
    }

    /// Construct a session **from a successful `/session` or `/signup` response**.
    ///
    /// - Reads the `SessionInfo` body (to learn the user pubky).
    /// - On native, selects `<pubky>=<secret>` from the saved `Set-Cookie` headers.
    pub(crate) async fn new_from_response(
        client: PubkyHttpClient,
        response: reqwest::Response,
    ) -> Result<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            // WASM: cookies are browser-managed; just parse the session body.
            let bytes = response.bytes().await?;
            let info = SessionInfo::deserialize(&bytes)?;
            cross_log!(info, "Hydrated WASM session for {}", info.public_key());
            Ok(Self { client, info })
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // 1) Snapshot all Set-Cookie header values before consuming the body.
            let mut raw_set_cookies = Vec::new();
            for val in &response.headers().get_all(reqwest::header::SET_COOKIE) {
                if let Ok(raw) = std::str::from_utf8(val.as_bytes()) {
                    raw_set_cookies.push(raw.to_owned());
                }
            }

            // 2) Read and parse the session body (this consumes the response).
            let bytes = response.bytes().await?;
            let info = SessionInfo::deserialize(&bytes)?;

            // 3) Find the session cookie
            // Support both legacy (name = pubkey) and new (name = UUID) formats
            // Prefer the UUID cookie if both exist
            let parsed_cookies: Vec<_> = raw_set_cookies
                .iter()
                .filter_map(|raw| cookie::Cookie::parse(raw.clone()).ok())
                .collect();

            // Try to find UUID-based cookie first (new format - preferred)
            let cookie = parsed_cookies
                .iter()
                .find(|c| {
                    // UUID format: name is not pubkey, value is 26 chars
                    c.value().len() == 26 && c.name() != info.public_key().to_string()
                })
                // Fallback to legacy format (name = pubkey)
                .or_else(|| {
                    parsed_cookies.iter().find(|c| {
                        c.name() == info.public_key().to_string() && c.value().len() == 26
                    })
                })
                .ok_or_else(|| AuthError::Validation("missing session cookie".into()))?;

            let cookie_tuple = (cookie.name().to_string(), cookie.value().to_string());

            cross_log!(info, "Hydrated native session for {}", info.public_key());
            Ok(Self {
                client,
                info,
                cookie: cookie_tuple,
            })
        }
    }

    /// Returns the session info
    #[must_use]
    pub const fn info(&self) -> &SessionInfo {
        &self.info
    }

    /// Returns a reference to the internal `PubkyHttpClient`
    /// Raw transport handle. No per-session cookie injection. Use `storage()` for
    /// authenticated, session-scoped requests.
    #[must_use]
    pub const fn client(&self) -> &PubkyHttpClient {
        &self.client
    }

    /// Round-trip the current session with the homeserver to verify it’s still valid.
    ///
    /// Returns:
    /// - `Ok(Some(session))` if the server recognizes and returns the session (still valid).
    /// - `Ok(None)` if the session no longer exists (expired/invalidated).
    /// - `Err(_)` for transport or server errors unrelated to validity.
    ///
    /// This does *not* mutate the session; it’s a sanity/validity check.
    ///
    /// # Errors
    /// - Propagates transport failures from the session endpoint.
    /// - Returns [`crate::errors::Error::Authentication`] if the homeserver rejects the request.
    pub async fn revalidate(&self) -> Result<Option<SessionInfo>> {
        cross_log!(info, "Revalidating session for {}", self.info.public_key());
        let response = self.send_revalidate_request().await?;
        if Self::session_missing(&response) {
            cross_log!(
                warn,
                "Session for {} no longer valid (404)",
                self.info.public_key()
            );
            return Ok(None);
        }
        let info = Self::parse_session_info(response).await?;
        cross_log!(info, "Session for {} remains valid", self.info.public_key());
        Ok(Some(info))
    }

    async fn send_revalidate_request(&self) -> Result<reqwest::Response> {
        self.storage()
            .request(Method::GET, "/session")
            .await?
            .send()
            .await
            .map_err(Error::from)
    }

    fn session_missing(response: &reqwest::Response) -> bool {
        response.status() == StatusCode::NOT_FOUND
    }

    async fn parse_session_info(response: reqwest::Response) -> Result<SessionInfo> {
        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        Ok(SessionInfo::deserialize(&bytes)?)
    }

    /// Sign out and invalidate this session server-side.
    ///
    /// - **On success:** the session is consumed (dropped).
    /// - **On failure:** you get `(Error, Self)` back so you can retry or inspect.
    ///
    /// # Errors
    /// - Returns the original [`crate::errors::Error`] alongside `self` when the transport
    ///   request fails or the homeserver responds with a non-success status.
    pub async fn signout(self) -> std::result::Result<(), (Error, Self)> {
        cross_log!(info, "Signing out session for {}", self.info.public_key());
        let resp = match self.storage().delete("/session").await {
            Ok(r) => r,
            Err(e) => return Err((e, self)),
        };
        if let Err(e) = check_http_status(resp).await {
            cross_log!(
                error,
                "Signout for {} failed: {}",
                self.info.public_key(),
                e
            );
            return Err((e, self));
        }
        cross_log!(info, "Session for {} signed out", self.info.public_key());
        Ok(()) // success => `self` is consumed
    }

    /// Create a **session-mode** Storage bound to this user session.
    ///
    /// - Relative paths (e.g. `"pub/my-cool-app/file"`) are resolved to **this** user.
    /// - Requests that target this user’s homeserver automatically carry the
    ///   session cookie.
    ///
    /// See [`SessionStorage`] for usage examples.
    #[must_use]
    pub fn storage(&self) -> SessionStorage {
        cross_log!(
            debug,
            "Creating session storage handle for {}",
            self.info.public_key()
        );
        SessionStorage::new(self)
    }
}

impl std::fmt::Debug for PubkySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("PubkySession");
        ds.field("client", &self.client);
        ds.field("info", &self.info);
        #[cfg(not(target_arch = "wasm32"))]
        ds.field("cookie", &"<redacted>");
        ds.finish()
    }
}
