use axum::{
    extract::State,
    http::{header, HeaderValue},
    response::IntoResponse,
};
use tower_cookies::Cookies;

use crate::{
    core::{err_if_user_is_invalid::get_user_or_http_error, extractors::PubkyHost, AppState},
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
    get_user_or_http_error(pubky.public_key(), &mut state.sql_db.pool().into(), false).await?;

    let sessions =
        crate::core::layers::authz::sessions_from_cookies(&state, &cookies, pubky.public_key())
            .await?;

    // Return the first session
    if let Some(session) = sessions.into_iter().next() {
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

    Err(HttpError::not_found())
}
pub async fn signout(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    let sessions =
        crate::core::layers::authz::sessions_from_cookies(&state, &cookies, pubky.public_key())
            .await
            .unwrap_or_default();

    for session in sessions {
        let _ = SessionRepository::delete(&session.secret, &mut state.sql_db.pool().into()).await;
    }

    // Idempotent Success Response (200 OK)
    Ok(())
}
