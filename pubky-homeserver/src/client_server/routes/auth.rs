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
    response::IntoResponse,
};
use axum_extra::extract::Host;
use bytes::Bytes;
use pkarr::PublicKey;
use pubky_common::{capabilities::Capability, session::Session};
use std::collections::HashMap;
use tower_cookies::{
    cookie::time::{Duration, OffsetDateTime},
    cookie::SameSite,
    Cookie, Cookies,
};

/// Creates a brand-new user if they do not exist, then logs them in by creating a session.
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
    let public_key = token.pubky();

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
    }

    // 4) Create the new user record
    let user = UserRepository::create(public_key, uexecutor!(tx)).await?;
    tx.commit().await?;

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
    let public_key = token.pubky();

    // 2) Ensure user *does* exist
    let user = get_user_or_http_error(public_key, &mut state.sql_db.pool().into(), false).await?;

    // 3) Create the session & set cookie
    create_session_and_cookie(&state, cookies, &host, &user, token.capabilities()).await
}

/// Creates and stores a session, sets the cookie, returns session as JSON/string.
async fn create_session_and_cookie(
    state: &AppState,
    cookies: Cookies,
    host: &str,
    user: &UserEntity,
    capabilities: &[Capability],
) -> HttpResult<impl IntoResponse> {
    let session_secret =
        SessionRepository::create(user.id, capabilities, &mut state.sql_db.pool().into()).await?;

    // 3) Build and set cookie
    let mut cookie = Cookie::new(user.public_key.to_string(), session_secret.to_string());
    cookie.set_path("/");
    if is_secure(host) {
        // Allow this cookie only to be sent over HTTPS.
        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
    }
    // Prevent javascript from accessing the cookie.
    cookie.set_http_only(true);
    // Set the cookie to expire in one year.
    let one_year = Duration::days(365);
    let expiry = OffsetDateTime::now_utc() + one_year;
    cookie.set_max_age(one_year);
    cookie.set_expires(expiry);
    cookies.add(cookie);

    let session = Session::new(&user.public_key, capabilities, None);
    Ok(session.serialize())
}

/// Determines if the host requires secure cookie attributes.
///
/// It's considered secure if the host is a pkarr public key or a fully-qualified
/// domain name (contains a dot). IP addresses and simple hostnames (like Docker
/// container names or localhost) are treated as non-secure development environments.
fn is_secure(host: &str) -> bool {
    // A pkarr public key is always a secure context.
    if PublicKey::try_from(host).is_ok() {
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
    use pkarr::Keypair;

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
        assert!(is_secure(&Keypair::random().public_key().to_string()));
        assert!(is_secure("example.com"));
    }
}
