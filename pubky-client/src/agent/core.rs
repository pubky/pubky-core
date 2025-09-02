use reqwest::{Method, StatusCode};
use std::sync::Arc;

use pubky_common::{auth::AuthToken, session::Session};

use crate::{PubkyClient, PublicKey, Result, util::check_http_status};

#[cfg(not(target_arch = "wasm32"))]
use crate::errors::AuthError;

/// Stateful, per-identity API driver built on a shared [`PubkyClient`].
///
/// An `PubkyAgent` represents one user/identity. It optionally holds a `Keypair` (for
/// self-signed flows like `signin()`/`signup()`), and always tracks the user’s `pubky`
/// (either from the keypair or learned later via the pubkyauth flow). On native targets,
/// each agent also owns exactly one session cookie secret; cookies never leak across agents.
///
/// What it does:
/// - Attaches the correct session cookie to requests that target this agent’s homeserver
///   (`pubky://<pubky>/...` or `https://_pubky.<pubky>/...`), and to nothing else.
/// - Exposes homeserver verbs (`get/put/post/patch/delete/head`) scoped to this identity.
/// - Implements identity flows: `signup`, `signin`, `signout`, `session`, and pubkyauth.
///
/// When to use:
/// - Use `PubkyAgent` whenever you’re acting “as a user” against a Pubky homeserver.
/// - Use `PubkyClient` only for raw transport or unauthenticated/public operations.
///
/// Concurrency:
/// - `PubkyAgent` is cheap to clone and thread-safe; it shares the underlying `PubkyClient`.
#[derive(Clone, Debug)]
pub struct PubkyAgent {
    pub(crate) client: Arc<PubkyClient>,

    /// Known session for this agent.
    pub(crate) session: Session,

    /// Native-only, single session cookie secret for `_pubky.<pubky>`. Never shared across agents.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cookie: String,
}

impl PubkyAgent {
    /// Establish a session from a signed [`AuthToken`].
    ///
    /// This POSTs `pubky://{user}/session` with the token, validates the response,
    /// and delegates construction to [`Self::new_from_response`].
    pub async fn new(client: Arc<PubkyClient>, token: &AuthToken) -> Result<PubkyAgent> {
        let url = format!("pubky://{}/session", token.pubky());
        let response = client
            .cross_request(Method::POST, url)
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;
        Self::new_from_response(client, response).await
    }

    /// Construct an agent **from a successful `/session` or `/signup` response**.
    ///
    /// - Reads the `Session` body (to learn the user pubky).
    /// - On native, selects `<pubky>=<secret>` from the saved `Set-Cookie` headers.
    pub(crate) async fn new_from_response(
        client: Arc<PubkyClient>,
        response: reqwest::Response,
    ) -> Result<PubkyAgent> {
        #[cfg(target_arch = "wasm32")]
        {
            // WASM: cookies are browser-managed; just parse the session body.
            let bytes = response.bytes().await?;
            let session = Session::deserialize(&bytes)?;
            return Ok(PubkyAgent { client, session });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // 1) Snapshot all Set-Cookie header values before consuming the body.
            let mut raw_set_cookies = Vec::new();
            for val in response
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
            {
                if let Ok(raw) = std::str::from_utf8(val.as_bytes()) {
                    raw_set_cookies.push(raw.to_owned());
                }
            }

            // 2) Read and parse the session body (this consumes the response).
            let bytes = response.bytes().await?;
            let session = Session::deserialize(&bytes)?;

            // 3) Find the cookie named exactly as the user's pubky.
            let cookie_name = session.pubky().to_string();
            let cookie = raw_set_cookies
                .iter()
                .filter_map(|raw| cookie::Cookie::parse(raw.clone()).ok())
                .find(|c| c.name() == cookie_name)
                .map(|c| c.value().to_string())
                .ok_or_else(|| AuthError::Validation("missing session cookie".into()))?;

            Ok(PubkyAgent {
                client,
                session,
                cookie,
            })
        }
    }

    /// Returns the agent public key
    pub fn pubky(&self) -> PublicKey {
        self.session.pubky().clone()
    }

    /// Returns the agent session
    pub fn session(&self) -> Session {
        self.session.clone()
    }

    /// Returns a reference to the internal `PubkyClient`
    /// Raw transport handle. No per-agent cookie injection. Use `homeserver()` for
    /// authenticated, agent-scoped requests.
    pub fn client(&self) -> &PubkyClient {
        Arc::as_ref(&self.client)
    }

    /// Retrieve session for current pubky from homeserver.
    pub async fn session_from_homeserver(&self) -> Result<Option<Session>> {
        let response = self
            .drive()
            .request(Method::GET, "/session")
            .await?
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Sign out and invalidate this agent’s server-side session. Consumes the agent.
    pub async fn signout(self) -> Result<()> {
        let response = self.drive().delete("/session").await?;
        check_http_status(response).await?;
        Ok(())
    }
}
