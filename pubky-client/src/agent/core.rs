use reqwest::Method;
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
    pub async fn new(client: Arc<PubkyClient>, token: &AuthToken) -> Result<PubkyAgent> {
        let url = format!("pubky://{}/session", token.pubky());
        let response = client
            .cross_request(Method::POST, url)
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;

        #[cfg(target_arch = "wasm32")]
        {
            // WASM: cookies handled by browser; just parse the session body.
            let bytes = response.bytes().await?;
            let session = Session::deserialize(&bytes)?;
            return Ok(PubkyAgent { client, session });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native: capture cookie before consuming the body.
            let cookie = Self::capture_session_cookie(token.pubky(), &response)
                .ok_or_else(|| AuthError::Validation("missing session cookie".into()))?;

            let bytes = response.bytes().await?;
            let session = Session::deserialize(&bytes)?;

            return Ok(PubkyAgent {
                client,
                session,
                cookie,
            });
        }
    }

    /// Returns the agent public key
    pub fn pubky(&self) -> PublicKey {
        self.session.pubky().clone()
    }

    /// Returns a reference to the internal `PubkyClient`
    /// Raw transport handle. No per-agent cookie injection. Use `homeserver()` for
    /// authenticated, agent-scoped requests.
    pub fn client(&self) -> &PubkyClient {
        Arc::as_ref(&self.client)
    }
}
