//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.
//!
//! Session management routes are provided by the auth module via
//! [`crate::client_server::auth::tenant_router`].
//! Write handlers call [`crate::client_server::auth::has_write_permission`]
//! to enforce capability-based write access control.

use axum::{extract::DefaultBodyLimit, routing::get, Router};

use crate::client_server::{auth::AuthenticationLayer, AppState};

pub mod read;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    // Data routes — public reads need no auth; write handlers gate on
    // `has_write_permission` after extracting the session.
    Router::new()
        .route(
            "/{*path}",
            get(read::get)
                .head(read::head)
                .put(write::put)
                .delete(write::delete),
        )
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        // XXX: dzdidi - WebDAV compliant auth. Which is actually http basic auth so we need some magic here
        // to make session based auth look like http auth while also accepting http basic auth for webDAV comp
        .layer(AuthenticationLayer::new(state.auth_state))
}
