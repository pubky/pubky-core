use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, head, put},
    Router,
};

use crate::core::{layers::authz::AuthorizationLayer, AppState};

pub mod read;
pub mod write;

pub fn data_store_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/pub/", get(read::list_root))
        .route("/pub/*path", get(read::get))
        .route("/pub/*path", head(read::head))
        .route("/pub/*path", put(write::put))
        .route("/pub/*path", delete(write::delete))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(AuthorizationLayer::new(state.clone()))
}
