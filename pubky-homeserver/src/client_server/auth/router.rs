//! Auth router constructors.
//!
//! The auth module provides two routers:
//! - [`base_router`]: Base-level routes (signup, signin) — no auth middleware.
//! - [`tenant_router`]: Tenant-level session management routes — caller applies
//!   [`AuthenticationLayer`].

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::client_server::AppState;

use super::routes::{grants, session, signin};

/// Base-level auth routes: signup and signin.
///
/// No authentication layer — these are entry points for creating sessions.
pub fn base_router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(signin::signup))
        .route("/session", post(signin::signin))
}

/// Tenant-level auth routes: session management.
///
/// Caller must apply [`AuthenticationLayer`] to resolve credentials.
pub fn tenant_router() -> Router<AppState> {
    Router::new()
        .route("/session", get(session::session).delete(session::signout))
        .route("/sessions", get(grants::list_grants))
        .route("/session/{gid}", delete(grants::revoke_grant))
}
