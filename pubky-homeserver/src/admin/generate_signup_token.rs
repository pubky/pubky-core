
// use crate::admin::admin_auth_middleware::AdminAuthLayer;
// use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};

// use super::app_state::AppState;

// pub async fn generate_signup_token(State(mut state): State<AppState>) -> anyhow::Result<impl IntoResponse> {
//     let token = state.db.generate_signup_token()?;
//     Ok((StatusCode::OK, token))
// }

// pub fn router(state: AppState) -> Router<AppState> {
//     let admin_password = state.password.as_str();
//     Router::new()
//         .route("/generate_signup_token", get(generate_signup_token))
//         .layer(AdminAuthLayer::new(admin_password))
// }
