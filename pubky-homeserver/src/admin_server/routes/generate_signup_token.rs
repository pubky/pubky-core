use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use crate::{
    data_directory::user_limit_config::UserLimitConfig,
    persistence::sql::signup_code::{SignupCodeId, SignupCodeRepository},
    shared::HttpResult,
};

use super::super::app_state::AppState;

/// Shared helper: create a signup code with optional custom limits.
async fn create_signup_code(
    state: &AppState,
    custom_limits: Option<&UserLimitConfig>,
) -> HttpResult<impl IntoResponse> {
    let code = SignupCodeRepository::create(
        &SignupCodeId::random(),
        custom_limits,
        &mut state.sql_db.pool().into(),
    )
    .await?;
    Ok((StatusCode::OK, code.id.0))
}

/// GET /generate_signup_token — create a signup token without custom limits.
pub async fn generate_signup_token(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    create_signup_code(&state, None).await
}

/// POST /generate_signup_token — create a signup token with custom limits.
///
/// All fields in the JSON body are optional. Omitted fields = unlimited.
pub async fn generate_signup_token_with_limits(
    State(state): State<AppState>,
    Json(config): Json<UserLimitConfig>,
) -> HttpResult<impl IntoResponse> {
    // Bandwidth budget strings are validated by BandwidthBudget deserialization —
    // invalid values cause a 422 from axum's Json extractor.
    create_signup_code(&state, Some(&config)).await
}
