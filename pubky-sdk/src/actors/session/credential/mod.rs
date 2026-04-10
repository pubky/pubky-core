//! Credential abstraction for [`PubkySession`](super::core::PubkySession).
//!
//! A credential is the per-identity material that authenticates one user
//! against one homeserver. The SDK supports two credential types:
//!
//! - **JWT** ([`jwt::JwtCredential`]) — the default. Long-lived user-signed
//!   grant + short-lived homeserver-minted access JWT, refreshed transparently.
//! - **Cookie** ([`cookie::CookieCredential`]) — legacy flow. A single opaque
//!   secret returned by `POST /session` and replayed via the `Cookie` header.
//!
//! [`SessionCredential`] is the port (Clean Architecture sense). All
//! session-aware code (`PubkySession`, `SessionStorage`) talks to the trait
//! and never matches on a credential variant. The two implementations live in
//! sibling modules so that retiring cookies later is a single-file delete.
//!
//! ## Why a trait, not an enum
//!
//! The previous design held a `Credential` enum and had `match` arms in every
//! method on `PubkySession` and `SessionStorage`. Adding a third credential
//! shape (or — more importantly — *removing* the cookie one) meant editing
//! every match. With a trait, those call sites become single virtual
//! dispatches and removing cookies becomes a one-file deletion.

pub(crate) mod cookie;
pub(crate) mod jwt;

use std::fmt::Debug;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use pubky_common::{crypto::PublicKey, session::SessionInfo};
use reqwest::{RequestBuilder, Response, StatusCode};

use crate::{PubkyHttpClient, errors::Result};

/// Shared `revalidate` helper: a `404` or `401` from the homeserver means
/// the credential is gone (revoked / expired), not a transport failure.
pub(crate) fn credential_session_missing(response: &Response) -> bool {
    matches!(
        response.status(),
        StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED
    )
}

pub(crate) use cookie::CookieCredential;
pub(crate) use jwt::JwtCredential;

/// Snapshot of the latest [`SessionInfo`], shared lock-free across clones.
///
/// Cookie credentials populate this once at construction. JWT credentials
/// update it on every successful refresh. Reads use a short `RwLock` read
/// guard and never block on the JWT refresh state.
pub(crate) type InfoSnapshot = Arc<RwLock<SessionInfo>>;

/// Behavior shared by every session credential type.
///
/// Implementations live in sibling modules:
/// - [`jwt::JwtCredential`] — grant + JWT (default)
/// - [`cookie::CookieCredential`] — legacy cookie flow
///
/// On native targets the boxed futures are `Send` so the trait is usable
/// behind `Arc<dyn SessionCredential>` from multi-threaded async runtimes.
/// On WASM (`wasm32`) the `?Send` variant is used because the browser event
/// loop is single-threaded and the underlying types contain `Rc`/`RefCell`.
// `async-trait` defaults to boxing futures as `Pin<Box<dyn Future + Send>>`,
// which doesn't compile on `wasm32-unknown-unknown` because the JS/WASM
// futures from `wasm-bindgen-futures` hold `Rc<RefCell<…>>` and aren't
// `Send`. The `?Send` variant drops that bound for WASM only; native keeps
// the `Send` bound so tokio's multi-threaded runtime stays happy.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub(crate) trait SessionCredential: Debug + Send + Sync {
    /// Cached [`SessionInfo`] snapshot. Cheap to read; never blocks on
    /// network or refresh state.
    fn info(&self) -> InfoSnapshot;

    /// Path used by `PubkySession::signout` against the user's homeserver.
    fn signout_path(&self) -> &'static str;

    /// Attach this credential's authentication to an in-flight request.
    ///
    /// JWT implementations may proactively refresh the bearer token here.
    /// Cookie implementations attach a `Cookie` header (or rely on the
    /// browser jar on WASM).
    async fn attach(
        &self,
        rb: RequestBuilder,
        client: &PubkyHttpClient,
    ) -> Result<RequestBuilder>;

    /// Round-trip the homeserver to verify this credential is still valid.
    ///
    /// Returns:
    /// - `Ok(Some(info))` — server recognised the credential.
    /// - `Ok(None)` — credential is gone (revoked / expired).
    /// - `Err(_)` — transport / server error unrelated to validity.
    async fn revalidate(
        &self,
        client: &PubkyHttpClient,
        user: &PublicKey,
    ) -> Result<Option<SessionInfo>>;

    /// Downcast accessor for JWT-only operations. Default implementation
    /// returns `None`; only [`JwtCredential`] overrides it.
    fn as_jwt(&self) -> Option<&JwtCredential> {
        None
    }

    /// Downcast accessor for cookie-only operations. Default implementation
    /// returns `None`; only [`CookieCredential`] overrides it.
    fn as_cookie(&self) -> Option<&CookieCredential> {
        None
    }
}
