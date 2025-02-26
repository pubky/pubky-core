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
    // Check for the admin password in a custom header "X-Admin-Password"
    let admin_password = state.admin.password.as_deref().unwrap_or("");
    if let Some(value) = headers.get("X-Admin-Password") {
        if value.to_str().unwrap_or("") != admin_password {
            return Err(Error::new(
                StatusCode::UNAUTHORIZED,
                Some("Invalid admin password"),
            ));
        }
    } else {
        return Err(Error::new(
            StatusCode::UNAUTHORIZED,
            Some("Missing admin password"),
        ));
    }
    // Generate a new signup token.
    let token = state.db.generate_signup_token()?;
    Ok((StatusCode::OK, token))
}
