//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.
//!
//! Session management routes rely on the [`AuthSession`] extractor for
//! authentication (401 if absent). Write handlers use the [`WriteAccess`]
//! extractor for capability-based write access control.

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get},
    Router,
};

use crate::client_server::{middleware::authentication::AuthenticationLayer, AppState};

pub mod grants;
pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    // Session management — authenticated via AuthSession extractor (401 if absent)
    let session_routes = Router::new()
        .route("/session", get(session::session).delete(session::signout))
        .route("/sessions", get(grants::list_grants))
        .route("/session/{gid}", delete(grants::revoke_grant));

    // Data routes — public reads need no auth, writes use WriteAccess extractor
    let data_routes = Router::new().route(
        "/{*path}",
        get(read::get)
            .head(read::head)
            .put(write::put)
            .delete(write::delete),
    );

    session_routes
        .merge(data_routes)
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        // XXX: dzdidi - WebDAV compliant auth. Which is actually http basic auth so we need some magic here
        // to make session based auth look like http auth while also accepting http basic auth for webDAV comp
        .layer(AuthenticationLayer::new(state.clone()))
}
