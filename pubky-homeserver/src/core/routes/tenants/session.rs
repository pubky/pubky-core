use axum::{
    extract::State,
    http::{header, HeaderValue},
    response::IntoResponse,
};
use tower_cookies::Cookies;

use crate::{
    core::{
        err_if_user_is_invalid::get_user_or_http_error, extractors::PubkyHost,
        layers::authz::session_secrets_from_cookies, AppState,
    },
    persistence::sql::session::SessionRepository,
    shared::{HttpError, HttpResult},
};

/// Return session information
/// Note: If there are multiple Cookies with valid sessions then only the first in the list will be returned.
pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    let user =
        get_user_or_http_error(pubky.public_key(), &mut state.sql_db.pool().into(), false).await?;

    // Try each session secret until we find one that belongs to this user
    for secret in session_secrets_from_cookies(&cookies) {
        if let Ok(session) =
            SessionRepository::get_by_secret(&secret, &mut state.sql_db.pool().into()).await
        {
            // Check if this session belongs to the requesting user
            if session.user_pubkey == user.public_key {
                let legacy_session = session.to_legacy();
                let mut resp = legacy_session.serialize().into_response();
                resp.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/octet-stream"),
                );
                resp.headers_mut()
                    .insert(header::VARY, HeaderValue::from_static("cookie, pubky-host"));
                resp.headers_mut().insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("private, must-revalidate"),
                );
                return Ok(resp);
            }
        };
    }

    Err(HttpError::not_found())
}
pub async fn signout(
    State(state): State<AppState>,
    cookies: Cookies,
) -> HttpResult<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    // Delete all sessions found in all cookies
    for secret in session_secrets_from_cookies(&cookies) {
        // Ignore errors - session might not exist in DB
        let _ = SessionRepository::delete(&secret, &mut state.sql_db.pool().into()).await;
    }

    // Idempotent Success Response (200 OK)
    Ok(())
}
