use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use crate::{
    data_directory::user_resource_quota::UserResourceQuota,
    persistence::sql::signup_code::{SignupCodeId, SignupCodeRepository},
    shared::{HttpError, HttpResult},
};

use super::super::app_state::AppState;

/// Shared helper: create a signup code with the given limits.
async fn create_signup_code(
    state: &AppState,
    limits: &UserResourceQuota,
) -> HttpResult<impl IntoResponse> {
    let code = SignupCodeRepository::create(
        &SignupCodeId::random(),
        limits,
        &mut state.sql_db.pool().into(),
    )
    .await?;
    Ok((StatusCode::OK, code.id.0))
}

/// GET /generate_signup_token — create a token with the server's deploy-time defaults.
///
/// The token carries `storage_quota_mb` from config, all other fields `Default`.
pub async fn generate_signup_token(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    create_signup_code(&state, &state.default_user_resource_quota).await
}

/// POST /generate_signup_token — create a token with explicit custom limits.
///
/// Accepts a partial JSON body:
/// - Absent fields → `Default` (use system default)
/// - `null` fields → `Unlimited` (explicitly no limit)
/// - Value fields → `Value(T)` (explicit limit)
pub async fn generate_signup_token_with_limits(
    State(state): State<AppState>,
    Json(config): Json<UserResourceQuota>,
) -> HttpResult<impl IntoResponse> {
    // Validate rate strings before touching the DB — return 422 for bad values.
    config
        .validate_rate_roundtrips()
        .map_err(|e| HttpError::new_with_message(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    create_signup_code(&state, &config).await
}
