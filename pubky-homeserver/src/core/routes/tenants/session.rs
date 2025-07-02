use axum::{extract::State, response::IntoResponse};
use tower_cookies::Cookies;

use crate::{
    core::{
        err_if_user_is_invalid::err_if_user_is_invalid, extractors::PubkyHost,
        layers::authz::session_secret_from_cookies, AppState,
    },
    shared::{HttpError, HttpResult},
};

pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
    let secret = match session_secret_from_cookies(&cookies, pubky.public_key()) {
        Some(secret) => secret,
        None => {
            tracing::warn!(
                "No session secret found in cookies for pubky {}",
                pubky.public_key()
            );
            return Err(HttpError::unauthorized_with_message(
                "No session secret found in cookies",
            ));
        }
    };

    if let Some(session) = state.db.get_session(&secret)? {
        // TODO: add content-type
        tracing::info!("Session found for provided secret {secret} for pubky {}. Session pubky: {}, created_at: {}", pubky.public_key(), session.pubky(), session.created_at());
        return Ok(session.serialize());
    }
    tracing::warn!(
        "Session not found for provided secret {} for pubky {}",
        secret,
        pubky.public_key()
    );
    Err(HttpError::not_found_with_message(
        "Session not found for provided secret",
    ))
}
pub async fn signout(
    State(mut state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        tracing::info!("Deleting session for pubky {}. Secret: {secret}", pubky.public_key());
        let deleted= state.db.delete_session(&secret)?;
        if !deleted {
            tracing::warn!("Can't delete session. Session not found for pubky {}. Secret: {secret}", pubky.public_key());
        }
    } else {
        tracing::warn!("Can't delete session. No session secret found in cookies for pubky {}", pubky.public_key());
    }

    // Idempotent Success Response (200 OK)
    Ok(())
}
