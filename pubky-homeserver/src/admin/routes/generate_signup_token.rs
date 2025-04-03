use crate::core::Result;
use axum::{extract::State, http::StatusCode, response::IntoResponse};

use super::super::app_state::AppState;

pub async fn generate_signup_token(State(mut state): State<AppState>) -> Result<impl IntoResponse> {
    let token = state.db.generate_signup_token()?;
    Ok((StatusCode::OK, token))
}
