//! Cookie-based authentication route handlers.
//!
//! Contains all cookie-specific handlers: signup, signin, get_session, signout.
//! Each handler is a full axum handler wired directly from `router.rs`.

use crate::persistence::sql::user::{UserEntity, UserRepository};
use crate::shared::{HttpError, HttpResult};
use crate::{client_server::auth::AuthState, client_server::middleware::pubky_host::PubkyHost};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    http::{header, HeaderValue},
    response::IntoResponse,
};
use axum_extra::extract::Host;
use bytes::Bytes;
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;
use pubky_common::session::CookieSessionRecord;
use std::collections::HashMap;
use tower_cookies::{
    cookie::time::{Duration, OffsetDateTime},
    cookie::SameSite,
    Cookie, Cookies,
};

use super::persistence::SessionRepository;

/// Creates a brand-new user if they do not exist, then logs them in by creating a session.
pub async fn signup(
    State(state): State<AuthState>,
    cookies: Cookies,
    Host(host): Host,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    let token = state.verifier.verify(&body)?;
    let signup_token = params.get("signup_token").map(|s| s.as_str());
    let user = state
        .auth_service
        .create_new_user(token.public_key(), &state.signup_mode, signup_token)
        .await?;
    create_session_and_cookie(&state, cookies, &host, &user, token.capabilities()).await
}

/// Creates and stores a session, sets the cookie, returns session as binary.
pub(crate) async fn create_session_and_cookie(
    state: &AuthState,
    cookies: Cookies,
    host: &str,
    user: &UserEntity,
    capabilities: &Capabilities,
) -> HttpResult<impl IntoResponse> {
    let session_secret =
        SessionRepository::create(user.id, capabilities, &mut state.sql_db.pool().into()).await?;

    let mut cookie = Cookie::new(user.public_key.z32(), session_secret.to_string());
    configure_session_cookie(&mut cookie, host);
    let one_year = Duration::days(365);
    let expiry = OffsetDateTime::now_utc() + one_year;
    cookie.set_max_age(one_year);
    cookie.set_expires(expiry);
    cookies.add(cookie);

    let record = CookieSessionRecord::new(&user.public_key, capabilities.clone(), None);
    let mut resp = record.serialize().into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(resp)
}

/// `POST /session` — sign in an existing user via deprecated AuthToken + cookie flow.
pub async fn signin(
    State(state): State<AuthState>,
    cookies: Cookies,
    Host(host): Host,
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    let token = state.verifier.verify(&body)?;
    let public_key = token.public_key();
    let user = UserRepository::get(public_key, &mut state.sql_db.pool().into())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => HttpError::not_found(),
            e => e.into(),
        })?;
    create_session_and_cookie(&state, cookies, &host, &user, token.capabilities()).await
}

/// `GET /session` — returns session info as postcard-serialized binary.
pub async fn get_session(
    auth: crate::client_server::auth::AuthSession,
) -> HttpResult<impl IntoResponse> {
    let crate::client_server::auth::AuthSession::Cookie(cookie_session) = auth else {
        return Err(HttpError::unauthorized());
    };
    let legacy_session = cookie_session.to_legacy();
    let mut resp = legacy_session.serialize().into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(resp)
}

/// `DELETE /session` — idempotently deletes the DB session (if any) and sets a removal cookie.
///
/// Takes `Option<AuthSession>` rather than `AuthSession` so a second signout with an
/// already-invalidated cookie is a 200 no-op rather than a 401. The removal cookie is
/// attached on both paths so the client always wipes any locally-stale cookie.
pub async fn signout(
    State(state): State<AuthState>,
    auth: Option<crate::client_server::auth::AuthSession>,
    cookies: Cookies,
    Host(host): Host,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    if let Some(crate::client_server::auth::AuthSession::Cookie(cookie_session)) = auth {
        SessionRepository::delete(&cookie_session.secret, &mut state.sql_db.pool().into()).await?;
    }

    let mut removal = Cookie::new(pubky.public_key().z32(), String::new());
    removal.make_removal();
    configure_session_cookie(&mut removal, &host);
    cookies.add(removal);

    Ok(StatusCode::OK.into_response())
}

pub(crate) fn configure_session_cookie(cookie: &mut Cookie<'static>, host: &str) {
    cookie.set_path("/");
    if is_secure(host) {
        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
    }
    cookie.set_http_only(true);
}

/// Determines if the host requires secure cookie attributes.
///
/// It's considered secure if the host is a pkarr public key or a fully-qualified
/// domain name (contains a dot). IP addresses and simple hostnames (like Docker
/// container names or localhost) are treated as non-secure development environments.
fn is_secure(host: &str) -> bool {
    if PublicKey::try_from_z32(host).is_ok() {
        return true;
    }

    url::Host::parse(host)
        .map(|host| match host {
            url::Host::Domain(domain) => domain.contains('.'),
            url::Host::Ipv4(_) | url::Host::Ipv6(_) => false,
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;
    use tower_cookies::cookie::SameSite;
    use tower_cookies::Cookie;

    use super::*;

    #[test]
    fn test_configure_session_cookie_secure() {
        let mut cookie = Cookie::new("key", "value");
        configure_session_cookie(&mut cookie, "example.com");
        assert!(cookie.secure().unwrap_or(false));
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.same_site(), Some(SameSite::None));
        assert_eq!(cookie.path(), Some("/"));
    }

    #[test]
    fn test_configure_session_cookie_insecure() {
        let mut cookie = Cookie::new("key", "value");
        configure_session_cookie(&mut cookie, "localhost");
        assert!(!cookie.secure().unwrap_or(false));
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.path(), Some("/"));
    }

    #[test]
    fn test_is_secure() {
        assert!(!is_secure(""));
        assert!(!is_secure("127.0.0.1"));
        assert!(!is_secure("homeserver"));
        assert!(!is_secure("testnet"));
        assert!(!is_secure("167.86.102.121"));
        assert!(!is_secure("[2001:0db8:0000:0000:0000:ff00:0042:8329]"));
        assert!(!is_secure("localhost"));
        assert!(!is_secure("localhost:23423"));
        assert!(is_secure(&Keypair::random().public_key().z32()));
        assert!(is_secure("example.com"));
    }
}
