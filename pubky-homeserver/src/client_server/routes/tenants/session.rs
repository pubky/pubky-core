use axum::{
    extract::State,
    http::{header, HeaderValue},
    response::IntoResponse,
    Json,
};
use axum_extra::extract::Host;
use pubky_common::auth::grant_session::GrantSessionInfo;
use tower_cookies::{Cookie, Cookies};

use crate::{
    client_server::{
        err_if_user_is_invalid::get_user_or_http_error,
        middleware::authentication::{session_secret_from_cookies, AuthSession},
        middleware::pubky_host::PubkyHost,
        routes::auth::configure_session_cookie,
        AppState,
    },
    persistence::sql::{
        grant::GrantRepository,
        grant_session::GrantSessionRepository,
        session::SessionRepository,
    },
    shared::{HttpError, HttpResult},
};

/// GET /session — return current session info.
///
/// For Bearer (grant): returns JSON `GrantSessionInfo`.
/// For Cookie (deprecated): returns postcard-serialized `SessionInfo`.
pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    get_user_or_http_error(pubky.public_key(), &mut state.sql_db.pool().into(), false).await?;

    // Try grant-based Bearer auth first (via extensions set by middleware)
    // Note: AuthSession may not be in extensions if the path was public-read
    // For GET /session, the middleware does insert it since it's a session management path.

    // Try deprecated cookie path
    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        if let Ok(session) =
            SessionRepository::get_by_secret(&secret, &mut state.sql_db.pool().into()).await
        {
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
        };
    }

    Err(HttpError::not_found())
}

/// GET /session with AuthSession — returns appropriate format based on auth method.
pub async fn session_with_auth(
    State(state): State<AppState>,
    auth: AuthSession,
    cookies: Cookies,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    match auth {
        AuthSession::Bearer(bearer) => {
            let grant = GrantRepository::get_by_grant_id(
                &bearer.grant_id,
                &mut state.sql_db.pool().into(),
            )
            .await
            .map_err(|_| HttpError::not_found())?;

            let info = GrantSessionInfo {
                homeserver: state.homeserver_keypair.public_key(),
                pubky: bearer.user_key,
                client_id: grant.client_id.clone(),
                capabilities: bearer.capabilities.to_vec(),
                grant_id: bearer.grant_id,
                token_expires_at: 0, // TODO: store in bearer session
                grant_expires_at: grant.expires_at as u64,
                created_at: grant.created_at.and_utc().timestamp() as u64,
            };
            Ok(Json(info).into_response())
        }
        AuthSession::Cookie(cookie_session) => {
            let legacy_session = cookie_session.session.to_legacy();
            let mut resp = legacy_session.serialize().into_response();
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            Ok(resp)
        }
    }
}

/// DELETE /session — sign out / revoke grant.
///
/// For Bearer (grant): revokes the underlying grant and all its sessions.
/// For Cookie (deprecated): deletes the session and removes the cookie.
pub async fn signout(
    State(state): State<AppState>,
    cookies: Cookies,
    Host(host): Host,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    // Check if we have a Bearer token (grant) via AuthSession in extensions
    // The middleware inserts AuthSession for session management paths

    // Try deprecated cookie signout (always works if cookie is present)
    if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
        SessionRepository::delete(&secret, &mut state.sql_db.pool().into()).await?;
    }

    // Always instruct the client to drop the session cookie
    let mut removal = Cookie::new(pubky.public_key().z32(), String::new());
    removal.make_removal();
    configure_session_cookie(&mut removal, &host);
    cookies.add(removal);

    Ok(())
}

/// DELETE /session with AuthSession — dispatches based on auth method.
pub async fn signout_with_auth(
    State(state): State<AppState>,
    auth: AuthSession,
    cookies: Cookies,
    Host(host): Host,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    match auth {
        AuthSession::Bearer(bearer) => {
            // Revoke the grant and delete all sessions for it
            GrantRepository::revoke(&bearer.grant_id, &mut state.sql_db.pool().into()).await?;
            GrantSessionRepository::delete_all_for_grant(
                &bearer.grant_id,
                &mut state.sql_db.pool().into(),
            )
            .await?;
            Ok(())
        }
        AuthSession::Cookie(_) => {
            // Deprecated cookie signout
            if let Some(secret) = session_secret_from_cookies(&cookies, pubky.public_key()) {
                SessionRepository::delete(&secret, &mut state.sql_db.pool().into()).await?;
            }
            let mut removal = Cookie::new(pubky.public_key().z32(), String::new());
            removal.make_removal();
            configure_session_cookie(&mut removal, &host);
            cookies.add(removal);
            Ok(())
        }
    }
}
