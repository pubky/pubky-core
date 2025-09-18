use axum::{extract::State, http::StatusCode, response::IntoResponse};

use crate::{
    persistence::sql::signup_code::{SignupCodeId, SignupCodeRepository},
    shared::HttpResult,
};

use super::super::app_state::AppState;

pub async fn generate_signup_token(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    let code =
        SignupCodeRepository::create(&SignupCodeId::random(), &mut state.sql_db.pool().into())
            .await?;
    Ok((StatusCode::OK, code.id.0))
}
