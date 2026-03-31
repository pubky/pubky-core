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

use super::cookie::routes::signup;
use super::jwt::routes;
use super::routes::{session, signin};
use crate::client_server::auth::AuthState;

/// Base-level auth routes: signup and signin.
///
/// No authentication layer — these are entry points for creating sessions.
pub fn base_router(auth_state: AuthState) -> Router<()> {
    Router::new()
        .route("/signup", post(signup))
        .route("/session", post(signin::signin))
        .with_state(auth_state)
}

/// Tenant-level auth routes: session management.
///
/// Caller must apply [`AuthenticationLayer`] to resolve credentials.
pub fn tenant_router(auth_state: AuthState) -> Router<()> {
    Router::new()
        .route("/session", get(session::session).delete(session::signout))
        .route("/sessions", get(routes::list_grants))
        .route("/session/{gid}", delete(routes::revoke_grant))
        .with_state(auth_state)
}
