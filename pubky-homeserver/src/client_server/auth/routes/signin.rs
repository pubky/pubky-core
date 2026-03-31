//! Signin dispatcher — routes between deprecated cookie auth and grant-based JWT auth
//! based on the Content-Type header.

use crate::shared::{HttpError, HttpResult};
use crate::client_server::{
    auth::AuthState,
    err_if_user_is_invalid::get_user_or_http_error,
};
use axum::{
    extract::State,
    http::StatusCode,
    http::header,
    response::IntoResponse,
};
use axum_extra::extract::Host;
use bytes::Bytes;
use tower_cookies::Cookies;

use crate::client_server::auth::cookie::routes::create_session_and_cookie;

/// Signs in an existing user. Dispatches between deprecated cookie and grant/JWT flows
/// based on the Content-Type header.
///
/// - `application/json` → grant-based auth (returns JWT)
/// - Otherwise → deprecated AuthToken-based auth (returns cookie)
pub async fn signin(
    State(state): State<AuthState>,
    cookies: Cookies,
    Host(host): Host,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    if is_json_content_type(&headers) {
        let request: crate::client_server::auth::jwt::routes::CreateGrantSessionRequest =
            serde_json::from_slice(&body).map_err(|e| {
                HttpError::new_with_message(
                    StatusCode::BAD_REQUEST,
                    format!("Invalid JSON body: {e}"),
                )
            })?;
        return crate::client_server::auth::jwt::routes::create_grant_session(
            State(state),
            axum::Json(request),
        )
        .await
        .map(|r| r.into_response());
    }

    // Deprecated flow: AuthToken + cookie
    let token = state.verifier.verify(&body)?;
    let public_key = token.public_key();
    let user = get_user_or_http_error(public_key, &mut state.sql_db.pool().into(), false).await?;
    create_session_and_cookie(&state, cookies, &host, &user, token.capabilities())
        .await
        .map(|r| r.into_response())
}

fn is_json_content_type(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use super::*;

    #[test]
    fn json_content_type_detected() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        assert!(is_json_content_type(&headers));
    }

    #[test]
    fn json_content_type_with_charset() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        assert!(is_json_content_type(&headers));
    }

    #[test]
    fn non_json_content_type() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        assert!(!is_json_content_type(&headers));
    }

    #[test]
    fn missing_content_type() {
        let headers = axum::http::HeaderMap::new();
        assert!(!is_json_content_type(&headers));
    }
}
