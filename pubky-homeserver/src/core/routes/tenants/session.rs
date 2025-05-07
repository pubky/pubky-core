use axum::{extract::State, http::StatusCode, response::IntoResponse};
use tower_cookies::Cookies;

use crate::core::{
    err_if_user_is_invalid::err_if_user_is_invalid, error::{Error, Result}, extractors::PubkyHost, sessions::session_secret_from_cookies, AppState
};

pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> Result<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db)?;
    let session = match state.session_manager.extract_session_from_cookies(&cookies) {
        Some(session) => session,
        None => {
            return Err(Error::with_status(StatusCode::NOT_FOUND));
        }
    };
    Ok(session.serialize())
}
pub async fn signout(
    State(mut state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> Result<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        state.db.delete_session(&secret)?;
    }

    // Idempotent Success Response (200 OK)
    Ok(())
}
