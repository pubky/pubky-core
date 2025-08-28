use axum::{extract::State, response::IntoResponse};
use tower_cookies::Cookies;

use crate::{
    client_server::{
        err_if_user_is_invalid::get_user_or_http_error, extractors::PubkyHost,
        layers::authz::session_secret_from_cookies, AppState,
    },
    persistence::sql::session::SessionRepository,
    shared::{HttpError, HttpResult},
};

pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    get_user_or_http_error(pubky.public_key(), &mut state.sql_db.pool().into(), false).await?;

    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        if let Ok(session) =
            SessionRepository::get_by_secret(&secret, &mut state.sql_db.pool().into()).await
        {
            let legacy_session = session.to_legacy();
            // TODO: add content-type
            return Ok(legacy_session.serialize());
        };
    }

    Err(HttpError::not_found())
}
pub async fn signout(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        SessionRepository::delete(&secret, &mut state.sql_db.pool().into()).await?;
    }

    // Idempotent Success Response (200 OK)
    Ok(())
}
