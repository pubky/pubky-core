use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use crate::{
    data_directory::user_quota::UserQuota,
    persistence::sql::signup_code::{SignupCodeId, SignupCodeRepository},
    shared::{HttpError, HttpResult},
};

use super::super::app_state::AppState;

/// Shared helper: create a signup code with the given limits.
async fn create_signup_code(state: &AppState, limits: &UserQuota) -> HttpResult<impl IntoResponse> {
    let code = SignupCodeRepository::create(
        &SignupCodeId::random(),
        limits,
        &mut state.sql_db.pool().into(),
    )
    .await?;
    Ok((StatusCode::OK, code.id.0))
}

/// GET /generate_signup_token — create a token with all-Default limits.
///
/// All fields start as `Default` — resolved at enforcement time from system config.
pub async fn generate_signup_token(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    create_signup_code(&state, &UserQuota::default()).await
}

/// POST /generate_signup_token — create a token with explicit custom limits.
///
/// Accepts a partial JSON body:
/// - Absent fields → `Default` (use system default)
/// - `null` fields → `Default` (use system default)
/// - Value fields → `Value(T)` (explicit limit)
pub async fn generate_signup_token_with_limits(
    State(state): State<AppState>,
    Json(config): Json<UserQuota>,
) -> HttpResult<impl IntoResponse> {
    // Validate rate strings before touching the DB — return 422 for bad values.
    config
        .validate_rate_roundtrips()
        .map_err(|e| HttpError::new_with_message(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    create_signup_code(&state, &config).await
}
