//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, head, put},
    Router,
};

use crate::core::{layers::{ensure_host_matches_user_layer::EnsureHostMatchesUserLayer, ensure_path_capabilities_layer::EnsurePathCapabilitiesLayer}, sessions::SessionRequiredLayer, AppState};

pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {    

    let open_routes = Router::new()
        .route("/pub/", get(read::get))
        .route("/pub/{*path}", get(read::get))
        .route("/pub/{*path}", head(read::head));

    let auth_routes = Router::new()
        .route("/pub/{*path}", put(write::put).layer(EnsureHostMatchesUserLayer::new()).layer(EnsurePathCapabilitiesLayer::new()))
        .route("/pub/{*path}", delete(write::delete).layer(EnsureHostMatchesUserLayer::new()).layer(EnsurePathCapabilitiesLayer::new()))
        .route("/session", get(session::get_session))
        .route("/session", delete(session::signout))
        .layer(SessionRequiredLayer::new(state.clone()));

    let combined = Router::new()
        .merge(open_routes)
        .merge(auth_routes)
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024));

    combined
}
