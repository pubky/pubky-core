use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use crate::{
    data_directory::user_limit_config::UserLimitConfig,
    persistence::sql::signup_code::{SignupCodeId, SignupCodeRepository},
    shared::HttpResult,
};

use super::user_limits::ExplicitUserLimitConfig;

use super::super::app_state::AppState;

/// Shared helper: create a signup code with the given limits.
async fn create_signup_code(
    state: &AppState,
    limits: &UserLimitConfig,
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
/// To create a token with explicit custom limits use the POST endpoint instead.
pub async fn generate_signup_token(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    create_signup_code(&state, &state.default_user_limits).await
}

/// POST /generate_signup_token — create a token with explicit custom limits.
///
/// All four fields are **required**. Use `null` for unlimited.
/// Omitting a field returns 422, preventing accidental unlimited grants.
pub async fn generate_signup_token_with_limits(
    State(state): State<AppState>,
    Json(explicit): Json<ExplicitUserLimitConfig>,
) -> HttpResult<impl IntoResponse> {
    let config: UserLimitConfig = explicit.into();
    create_signup_code(&state, &config).await
}
