//! Auth router constructors.
//!
//! The auth module provides two routers:
//! - [`base_router`]: Unauthenticated routes (signup, session creation).
//! - [`tenant_router`]: Authenticated session management routes.
//!
//! Cookie and JWT routes are fully separated — each has its own URL namespace
//! and its own auth-method-specific middleware.

use axum::{
    routing::{delete, get, post},
    Router,
};

use super::cookie;
use super::jwt;
use crate::client_server::auth::cookie::middleware::CookieAuthenticationLayer;
use crate::client_server::auth::jwt::middleware::JwtAuthenticationLayer;
use crate::client_server::auth::AuthState;

/// Base-level auth routes — no authentication middleware.
///
/// These are entry points for creating sessions:
/// - `POST /signup` — cookie-based user creation (deprecated)
/// - `POST /session` — cookie-based signin (deprecated)
/// - `POST /auth/jwt/session` — grant-based JWT session creation
pub fn base_router(auth_state: AuthState) -> Router<()> {
    Router::new()
        // Cookie (deprecated)
        .route("/signup", post(cookie::routes::signup))
        .route("/session", post(cookie::routes::signin))
        // JWT
        .route(
            "/auth/jwt/session",
            post(jwt::routes::create_grant_session),
        )
        .with_state(auth_state)
}

/// Tenant-level auth routes — with per-method authentication middleware.
///
/// Cookie routes use [`CookieAuthenticationLayer`] (ignores Bearer tokens).
/// JWT routes use [`JwtAuthenticationLayer`] (ignores cookies).
pub fn tenant_router(auth_state: AuthState) -> Router<()> {
    let cookie_routes = Router::new()
        .route(
            "/session",
            get(cookie::routes::get_session).delete(cookie::routes::signout),
        )
        .layer(CookieAuthenticationLayer::new(auth_state.clone()));

    let jwt_routes = Router::new()
        .route(
            "/auth/jwt/session",
            get(jwt::routes::get_session).delete(jwt::routes::signout),
        )
        .route("/auth/jwt/sessions", get(jwt::routes::list_grants))
        .route(
            "/auth/jwt/session/{gid}",
            delete(jwt::routes::revoke_grant),
        )
        .layer(JwtAuthenticationLayer::new(auth_state.clone()));

    cookie_routes
        .merge(jwt_routes)
        .with_state(auth_state)
}
