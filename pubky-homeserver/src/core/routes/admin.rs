use crate::core::{error::Result, layers::admin::AdminAuthLayer, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};

pub async fn generate_signup_token(State(mut state): State<AppState>) -> Result<impl IntoResponse> {
    let token = state.db.generate_signup_token()?;
    Ok((StatusCode::OK, token))
}

pub fn router(state: AppState) -> Router<AppState> {
    let admin_password = state.admin.password.unwrap_or_default();
    Router::new()
        .route("/generate_signup_token", get(generate_signup_token))
        .layer(AdminAuthLayer::new(admin_password))
}
