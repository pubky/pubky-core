//! Capability views — type-safe access to credential-specific operations.
//!
//! [`PubkySession`] is a unified handle that does not know (at the type
//! level) which credential it carries. Most methods (`info`, `storage`,
//! `signout`, `revalidate`) work for both credential shapes. The
//! credential-specific operations live behind these views:
//!
//! - [`JwtSessionView`] — `list_grants`, `revoke_grant`, `current_jwt`,
//!   `grant_id`, `force_refresh`. Reachable via [`PubkySession::as_jwt`].
//! - [`CookieSessionView`] — `export_secret`, `write_secret_file`. Reachable
//!   via [`PubkySession::as_cookie`].
//!
//! Both views are zero-cost borrows tied to the session's lifetime. The
//! returning accessors (`as_jwt` / `as_cookie`) yield `None` when the
//! credential is the wrong shape, eliminating the runtime
//! `require_jwt_handle()` checks and the `export_secret()` panic that the
//! previous design carried.

use pubky_common::auth::{
    grant_session::GrantInfo,
    jws::GrantId,
};
use reqwest::Method;

use super::core::PubkySession;
use super::credential::{CookieCredential, JwtCredential};
use crate::actors::storage::resource::resolve_pubky;
use crate::errors::{RequestError, Result};
use crate::util::check_http_status;

// =====================================================================
// JWT view
// =====================================================================

/// JWT-only operations on a [`PubkySession`].
///
/// Obtain via [`PubkySession::as_jwt`]. The view borrows the session, so it
/// cannot outlive it; this is what makes the JWT-only API impossible to
/// misuse against a cookie session.
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

// =====================================================================
// Cookie view
// =====================================================================
//
// Available on every target. The view's surface narrows on browser WASM
// because the runtime cookie jar holds the secret and JavaScript cannot
// read it (the WHATWG fetch spec hides `Set-Cookie` from clients):
// `export_secret` returns `None` in that case. On native and Node.js WASM
// the SDK owns the secret and `export_secret` always returns `Some`.

/// Cookie-only operations on a [`PubkySession`].
///
/// Obtain via [`PubkySession::as_cookie`]. The view exists on every
/// target; whether the cookie secret is *available* depends on the
/// runtime — see [`Self::export_secret`].
#[derive(Debug)]
pub struct CookieSessionView<'a> {
    session: &'a PubkySession,
    credential: &'a CookieCredential,
}

impl<'a> CookieSessionView<'a> {
    pub(crate) const fn new(
        session: &'a PubkySession,
        credential: &'a CookieCredential,
    ) -> Self {
        Self {
            session,
            credential,
        }
    }

    /// Export the minimum data needed to restore this session later.
    /// Returns a single compact secret token `<pubkey>:<cookie_secret>`.
    ///
    /// Useful for scripts that need restarting. Helps avoid a new auth
    /// flow from a signer on a script restart.
    ///
    /// Treat the returned String as a **bearer secret**. Do not log it;
    /// store it securely.
    ///
    /// # Returns
    /// - `Some(token)` on native and Node.js WASM (the SDK captured the
    ///   secret from `Set-Cookie`).
    /// - `None` on browser WASM, where `Set-Cookie` is hidden from
    ///   JavaScript by the WHATWG fetch spec — only the browser cookie
    ///   jar holds the value.
    #[must_use]
    pub fn export_secret(&self) -> Option<String> {
        let public_key = self.session.info().public_key().z32();
        let cookie = self.credential.cookie_secret()?;
        crate::cross_log!(info, "Exporting session secret for {}", public_key);
        Some(format!("{public_key}:{cookie}"))
    }

    /// Write the session secret token to disk. Ensures a `.sess` extension.
    /// Native-only — depends on the standard filesystem APIs.
    ///
    /// Behavior:
    /// - If `secret_file_path` already ends with `.sess`, it is used as-is.
    /// - If it has no extension, `.sess` is added.
    /// - If it has a different extension, the filename gains `.sess` (e.g.,
    ///   `foo.txt` -> `foo.sess`).
    ///
    /// On Unix, permissions are set to `0o600`.
    /// # Errors
    /// - Returns [`std::io::Error`] if the file cannot be written or
    ///   permissions cannot be set. On native the secret is always
    ///   present, so this never errors with `NotFound` for that reason.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_secret_file<P: AsRef<std::path::Path>>(
        &self,
        secret_file_path: P,
    ) -> std::io::Result<()> {
        let token = self.export_secret().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no cookie secret captured for this session",
            )
        })?;
        let p = secret_file_path.as_ref();

        let target = match p.extension().and_then(|e| e.to_str()) {
            Some("sess") => p.to_path_buf(),
            Some(_) => {
                let mut out = p.to_path_buf();
                let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("session");
                out.set_file_name(format!("{fname}.sess"));
                out
            }
            None => {
                let mut out = p.to_path_buf();
                out.set_extension("sess");
                out
            }
        };

        std::fs::write(&target, token)?;
        crate::cross_log!(
            info,
            "Wrote session secret for {} to {}",
            self.session.info().public_key(),
            target.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))?;
        };
        Ok(())
    }
}
