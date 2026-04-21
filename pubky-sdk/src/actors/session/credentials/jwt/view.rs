//! JWT-only capability view — type-safe access to JWT-specific operations.
//!
//! [`JwtSessionView`] is obtained via
//! [`PubkySession::as_jwt`](crate::actors::session::core::PubkySession::as_jwt).
//! The view borrows the session, so it cannot outlive it; this is what makes
//! the JWT-only API impossible to misuse against a cookie session.

use pubky_common::auth::{
    grant_session::{GrantInfo, GrantSessionInfo},
    jws::GrantId,
};
use reqwest::Method;

use super::credential::JwtCredential;
use crate::actors::session::core::PubkySession;
use crate::actors::storage::resource::resolve_pubky;
use crate::errors::{RequestError, Result};
use crate::util::check_http_status;

/// JWT-only operations on a [`PubkySession`].
#[derive(Debug)]
pub struct JwtSessionView<'a> {
    session: &'a PubkySession,
    credential: &'a JwtCredential,
}

impl<'a> JwtSessionView<'a> {
    pub(crate) const fn new(session: &'a PubkySession, credential: &'a JwtCredential) -> Self {
        Self {
            session,
            credential,
        }
    }

    /// Returns the full JWT session metadata from the homeserver.
    ///
    /// This gives access to JWT-specific fields like `grant_id`,
    /// `client_id`, `token_expires_at`, and `grant_expires_at` that are
    /// not available via the shared
    /// [`PubkySession::info`](crate::actors::session::core::PubkySession::info)
    /// accessor.
    pub async fn session_info(&self) -> GrantSessionInfo {
        self.credential.state.lock().await.session.clone()
    }

    /// List all active grants for this user.
    ///
    /// Calls `GET /auth/jwt/sessions`. Requires the underlying session to
    /// have the **root** capability — non-root sessions get `403 Forbidden`
    /// from the homeserver.
    ///
    /// # Errors
    /// - Propagates HTTP errors from the homeserver (`401`/`403` for invalid
    ///   auth or missing root capability).
    pub async fn list_grants(&self) -> Result<Vec<GrantInfo>> {
        let (user, bearer) = {
            let g = self.credential.state.lock().await;
            (g.grant_claims.iss.clone(), g.jwt.clone())
        };
        let url = format!("pubky://{}/auth/jwt/sessions", user.z32());
        let resolved = resolve_pubky(&url)?;
        let resp = self
            .session
            .client()
            .cross_request(Method::GET, resolved)
            .await?
            .bearer_auth(&bearer)
            .send()
            .await?;
        let resp = check_http_status(resp).await?;
        let grants: Vec<GrantInfo> = resp.json().await.map_err(|e| RequestError::DecodeJson {
            message: format!("decoding /auth/jwt/sessions response: {e}"),
        })?;
        Ok(grants)
    }

    /// Revoke a specific grant by id, killing all of its sessions.
    ///
    /// Calls `DELETE /auth/jwt/session/{gid}`. Requires the **root**
    /// capability on this session.
    ///
    /// # Errors
    /// - Propagates HTTP errors from the homeserver (`401`/`403` for invalid
    ///   auth or missing root capability).
    pub async fn revoke_grant(&self, grant_id: &GrantId) -> Result<()> {
        let (user, bearer) = {
            let g = self.credential.state.lock().await;
            (g.grant_claims.iss.clone(), g.jwt.clone())
        };
        let url = format!(
            "pubky://{}/auth/jwt/session/{}",
            user.z32(),
            grant_id.as_str()
        );
        let resolved = resolve_pubky(&url)?;
        let resp = self
            .session
            .client()
            .cross_request(Method::DELETE, resolved)
            .await?
            .bearer_auth(&bearer)
            .send()
            .await?;
        check_http_status(resp).await?;
        Ok(())
    }

    /// Returns the current access JWT for this session.
    pub async fn current_jwt(&self) -> String {
        self.credential.current_jwt().await
    }

    /// Returns the grant id (`jti`) backing this session, for callers that
    /// need to revoke or display it.
    pub async fn grant_id(&self) -> GrantId {
        self.credential.state.lock().await.grant_claims.jti.clone()
    }

    /// Test/debug helper: force a refresh of the JWT credential right now.
    ///
    /// Used by integration tests to verify that a refresh produces a token
    /// with a fresh `iat`/`jti`. Returns the new token's `iat` for assertions.
    ///
    /// Bypasses the proactive-refresh time check so the refresh always runs.
    ///
    /// # Errors
    /// - Propagates HTTP errors from the refresh exchange.
    #[doc(hidden)]
    pub async fn force_refresh(&self) -> Result<u64> {
        // Bypass the proactive-refresh time check by setting `claims.exp`
        // to "expired now"; the refresh helper then always hits the network.
        self.credential.state.lock().await.claims.exp = 0;
        self.credential.refresh(self.session.client()).await?;
        let g = self.credential.state.lock().await;
        Ok(g.claims.iat)
    }
}
