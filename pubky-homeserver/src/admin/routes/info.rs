use super::super::app_state::AppState;
use crate::persistence::sql::signup_code::SignupCodeRepository;
use crate::persistence::sql::user::UserRepository;
use crate::shared::HttpResult;
use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct InfoResponse {
    num_users: u64,
    num_disabled_users: u64,
    total_disk_used_mb: u64,
    num_signup_codes: u64,
    num_unused_signup_codes: u64,
}

/// Return summary statistics about the homeserver.
pub async fn info(State(state): State<AppState>) -> HttpResult<(StatusCode, Json<InfoResponse>)> {
    let user_overview = UserRepository::get_overview(&mut state.sql_db.pool().into()).await?;
    let signup_code_overview =
        SignupCodeRepository::get_overview(&mut state.sql_db.pool().into()).await?;

    // Build response
    let body = InfoResponse {
        num_users: user_overview.count,
        num_disabled_users: user_overview.disabled_count,
        total_disk_used_mb: user_overview.total_used_mb,
        num_signup_codes: signup_code_overview.num_signup_codes as u64,
        num_unused_signup_codes: signup_code_overview.num_unused_signup_codes as u64,
    };

    Ok((StatusCode::OK, Json(body)))
}
