//! Authentication route handlers (signup and signin).
//!
//! Both flows verify a client-provided `AuthToken` (public key + signature),
//! create or look up the user, and return a session cookie. Signup may
//! additionally require a signup token depending on server configuration.

use crate::persistence::sql::{
    session::SessionRepository,
    signup_code::{SignupCodeId, SignupCodeRepository},
    uexecutor,
    user::{UserEntity, UserRepository},
};
use crate::shared::{HttpError, HttpResult};
use crate::{
    client_server::{err_if_user_is_invalid::get_user_or_http_error, AppState},
    SignupMode,
};
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
use pubky_common::session::SessionInfo;
use std::collections::HashMap;
use tower_cookies::{
    cookie::time::{Duration, OffsetDateTime},
    cookie::SameSite,
    Cookie, Cookies,
};

use crate::data_directory::user_limit_config::UserLimitConfig;

/// How long a session cookie is valid before expiring.
const SESSION_EXPIRY_DAYS: i64 = 365;

/// Create a user record with explicit limits and commit the transaction.
async fn create_user_with_limits(
    public_key: &PublicKey,
    limits: &UserLimitConfig,
    mut tx: sqlx::Transaction<'static, sqlx::Postgres>,
) -> HttpResult<UserEntity> {
    let mut user = UserRepository::create(public_key, uexecutor!(tx)).await?;
    UserRepository::set_custom_limits(user.id, limits, uexecutor!(tx)).await?;
    user.apply_custom_limits(limits);
    tx.commit().await?;
    Ok(user)
}

/// Creates a brand-new user if they do not exist, then logs them in by creating a session.
///
/// Note: This endpoint uses action-oriented path `/signup` which is not RESTful.
/// Ideally would be eg `POST /users` in future.
///
/// 1) Check if signup tokens are required (signup mode is token_required).
/// 2) Ensure the user *does not* already exist.
/// 3) Create new user if needed.
/// 4) Create a session and set the cookie (using the shared helper).
pub async fn signup(
    State(state): State<AppState>,
    cookies: Cookies,
    Host(host): Host,
    Query(params): Query<HashMap<String, String>>, // for extracting `signup_token` if needed
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    // 1) Verify AuthToken from request body
    let token = state.verifier.verify(&body)?;
    let public_key = token.public_key();

    let mut tx = state.sql_db.pool().begin().await?;
    // 2) Ensure the user does *not* already exist
    match UserRepository::get(public_key, uexecutor!(tx)).await {
        Ok(_) => {
            return Err(HttpError::new_with_message(
                StatusCode::CONFLICT,
                "User already exists",
            ));
        }
        Err(sqlx::Error::RowNotFound) => {
            // User does not exist, continue
        }
        Err(e) => {
            return Err(e.into());
        }
    }

    // 3) If signup_mode == token_required, require & validate a `signup_token` param.
    if state.signup_mode == SignupMode::TokenRequired {
        let signup_token_param = params
            .get("signup_token")
            .ok_or(HttpError::new_with_message(
                StatusCode::BAD_REQUEST,
                "Token required",
            ))?;
        let signup_code_id = SignupCodeId::new(signup_token_param.clone()).map_err(|e| {
            HttpError::new_with_message(
                StatusCode::BAD_REQUEST,
                format!("Invalid signup token format: {}", e),
            )
        })?;

        // Validate it in the DB (marks it used)
        let code = match SignupCodeRepository::get(&signup_code_id, uexecutor!(tx)).await {
            Ok(code) => code,
            Err(sqlx::Error::RowNotFound) => {
                return Err(HttpError::new_with_message(
                    StatusCode::UNAUTHORIZED,
                    "Invalid token",
                ));
            }
            Err(e) => {
                return Err(e.into());
            }
        };

        if code.used_by.is_some() {
            return Err(HttpError::new_with_message(
                StatusCode::UNAUTHORIZED,
                "Token already used",
            ));
        }

        SignupCodeRepository::mark_as_used(&signup_code_id, public_key, uexecutor!(tx)).await?;

        // 4) Create the new user record with the token's limits.
        let limits = code.custom_limits().unwrap_or_default();
        let user = create_user_with_limits(public_key, &limits, tx).await?;
        return create_session_and_cookie(&state, cookies, &host, &user, token.capabilities())
            .await;
    }

    // 4) Create the new user record (open signup, no token).
    let user = create_user_with_limits(public_key, &state.default_user_limits, tx).await?;

    // 5) Create session & set cookie
    create_session_and_cookie(&state, cookies, &host, &user, token.capabilities()).await
}

/// Fails if user doesn’t exist, otherwise logs them in by creating a session.
pub async fn signin(
    State(state): State<AppState>,
    cookies: Cookies,
    Host(host): Host,
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    // 1) Verify the AuthToken in the request body
    let token = state.verifier.verify(&body)?;
    let public_key = token.public_key();

    // 2) Ensure user *does* exist
    let user = get_user_or_http_error(public_key, &mut state.sql_db.pool().into(), false).await?;

    // 3) Create the session & set cookie
    create_session_and_cookie(&state, cookies, &host, &user, token.capabilities()).await
}

/// Creates and stores a session, sets the cookie, returns session as JSON/string.
///
/// Uses a transaction with `FOR UPDATE` on the user row to serialize concurrent
/// session creation and prevent the max_sessions limit from being bypassed.
async fn create_session_and_cookie(
    state: &AppState,
    cookies: Cookies,
    host: &str,
    user: &UserEntity,
    capabilities: &Capabilities,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.sql_db.pool().begin().await?;

    if let Some(max) = user.limits().max_sessions {
        // Lock the user row for the duration of this transaction to serialize
        // concurrent session creation attempts for the same user.
        sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;

        let count = SessionRepository::count_by_user_id(user.id, uexecutor!(tx)).await?;
        if count >= i64::from(max) {
            return Err(HttpError::new_with_message(
                StatusCode::TOO_MANY_REQUESTS,
                format!("Maximum sessions ({max}) reached"),
            ));
        }
    }

    let session_secret = SessionRepository::create(user.id, capabilities, uexecutor!(tx)).await?;
    tx.commit().await?;

    // 3) Build and set cookie
    let mut cookie = Cookie::new(user.public_key.z32(), session_secret.to_string());
    configure_session_cookie(&mut cookie, host);
    let one_year = Duration::days(SESSION_EXPIRY_DAYS);
    let expiry = OffsetDateTime::now_utc() + one_year;
    cookie.set_max_age(one_year);
    cookie.set_expires(expiry);
    cookies.add(cookie);

    let session = SessionInfo::new(&user.public_key, capabilities.clone(), None);
    let mut resp = session.serialize().into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(resp)
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
    // A pkarr public key is always a secure context.
    if PublicKey::try_from_z32(host).is_ok() {
        return true;
    }

    // Fallback to parsing as a regular host.
    url::Host::parse(host)
        .map(|host| match host {
            // A domain is secure only if it's a FQDN (contains a dot).
            url::Host::Domain(domain) => domain.contains('.'),
            // Treat all direct IP addresses as non-secure for local/test setups.
            url::Host::Ipv4(_) | url::Host::Ipv6(_) => false,
        })
        .unwrap_or(false) // Default to non-secure on parsing failure.
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use super::*;

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
