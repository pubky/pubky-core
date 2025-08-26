use axum::{extract::State, response::IntoResponse};
use tower_cookies::Cookies;

use crate::{
    core::{
        err_if_user_is_invalid::{get_user_or_http_error}, extractors::PubkyHost,
        layers::authz::session_secret_from_cookies, AppState,
    },
    shared::{HttpError, HttpResult},
};

pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    get_user_or_http_error(pubky.public_key(), &mut (&mut state.sql_db.pool().into()), false).await?;

    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        if let Some(session) = state.db.get_session(&secret)? {
            // TODO: add content-type
            return Ok(session.serialize());
        };
    }

    Err(HttpError::not_found())
}
pub async fn signout(
    State(mut state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        state.db.delete_session(&secret)?;
    }

    // Idempotent Success Response (200 OK)
    Ok(())
}
