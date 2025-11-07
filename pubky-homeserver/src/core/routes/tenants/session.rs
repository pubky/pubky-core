use axum::{
    extract::State,
    http::{header, HeaderValue},
    response::IntoResponse,
};
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
    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        if let Some(session) = state.db.get_session(&secret)? {
            let mut resp = session.serialize().into_response();
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
