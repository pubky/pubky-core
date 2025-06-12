use axum::{extract::State, http::StatusCode, response::IntoResponse};

use crate::shared::HttpResult;

use super::super::app_state::AppState;

pub async fn generate_signup_token(
    State(mut state): State<AppState>,
) -> HttpResult<impl IntoResponse> {
    let token = state.db.generate_signup_token()?;
    Ok((StatusCode::OK, token))
}
