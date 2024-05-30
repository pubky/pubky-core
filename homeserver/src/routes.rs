use axum::{
    routing::{get, put},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::server::AppState;

// pub mod auth;
// mod protected;
// mod restricted;
pub mod root;
pub mod users;

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root::handler))
        // TODO: greatly rate limit this function from IPs
        .route("/register", put(users::register))
        // .route("/authn", get(auth::authn))
        .with_state(state)
        .layer(CorsLayer::very_permissive())
        .layer(CookieManagerLayer::new())
        .layer(TraceLayer::new_for_http())
}
