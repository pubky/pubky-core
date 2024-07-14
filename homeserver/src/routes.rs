use axum::{routing::get, Router};

pub mod root;

pub fn create_app() -> Router {
    Router::new().route("/", get(root::handler))
}
