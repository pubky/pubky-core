use crate::core::{
    error::{Error, Result},
    AppState,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

pub async fn generate_signup_token(
    State(mut state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    // 1) If no admin password is configured, block entirely the admin endpoints.
    let admin_password = match &state.admin.password {
        Some(pw) => pw,
        None => {
            return Err(Error::new(
                StatusCode::FORBIDDEN,
                Some("No admin password set. Admin endpoints are disabled."),
            ));
        }
    };

    // 2) If a password is set, require "X-Admin-Password" to match.
    match headers.get("X-Admin-Password") {
        Some(value) if value.to_str().unwrap_or_default() == admin_password => {
            // OK, proceed
        }
        Some(_) => {
            return Err(Error::new(
                StatusCode::UNAUTHORIZED,
                Some("Invalid admin password"),
            ));
        }
        None => {
            return Err(Error::new(
                StatusCode::UNAUTHORIZED,
                Some("Missing admin password"),
            ));
        }
    }

    // 3) Generate a new signup token
    let token = state.db.generate_signup_token()?;
    Ok((StatusCode::OK, token))
}
