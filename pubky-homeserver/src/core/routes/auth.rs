use crate::core::err_if_user_is_invalid::err_if_user_is_invalid;
use crate::persistence::lmdb::tables::signup_tokens::SignupTokenError;
use crate::persistence::lmdb::tables::users::User;
use crate::shared::{HttpError, HttpResult};
use crate::{core::AppState, SignupMode};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::{extract::Host, headers::UserAgent, TypedHeader};
use base32::{encode, Alphabet};
use bytes::Bytes;
use pkarr::PublicKey;
use pubky_common::capabilities::Capabilities;
use pubky_common::{crypto::random_bytes, session::SessionInfo};
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
    user_agent: Option<TypedHeader<UserAgent>>,
    cookies: Cookies,
    Host(host): Host,
    Query(params): Query<HashMap<String, String>>, // for extracting `signup_token` if needed
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    // 1) Verify AuthToken from request body
    let token = state.verifier.verify(&body)?;
    let public_key = token.public_key();

    // 2) Ensure the user does *not* already exist
    let txn = state.db.env.read_txn()?;
    let users = state.db.tables.users;
    if users.get(&txn, public_key)?.is_some() {
        return Err(HttpError::new_with_message(
            StatusCode::CONFLICT,
            "User already exists",
        ));
    }
    txn.commit()?;

    // 3) If signup_mode == token_required, require & validate a `signup_token` param.
    if state.signup_mode == SignupMode::TokenRequired {
        let signup_token_param = params
            .get("signup_token")
            .ok_or(HttpError::new_with_message(
                StatusCode::BAD_REQUEST,
                "Token required",
            ))?;
        // Validate it in the DB (marks it used)
        if let Err(e) = state
            .db
            .validate_and_consume_signup_token(signup_token_param, public_key)
        {
            tracing::warn!("Failed to signup. Invalid signup token: {:?}", e);
            match e {
                SignupTokenError::AlreadyUsed => {
                    return Err(HttpError::new_with_message(
                        StatusCode::UNAUTHORIZED,
                        "Token already used",
                    ));
                }
                SignupTokenError::InvalidToken => {
                    return Err(HttpError::new_with_message(
                        StatusCode::UNAUTHORIZED,
                        "Invalid token",
                    ));
                }
                SignupTokenError::DatabaseError(e) => {
                    return Err(e.into());
                }
            }
        }
    }

    // 4) Create the new user record
    let mut wtxn = state.db.env.write_txn()?;
    users.put(&mut wtxn, public_key, &User::default())?;
    wtxn.commit()?;

    // 5) Create session & set cookie
    create_session_and_cookie(
        &state,
        cookies,
        &host,
        public_key,
        token.capabilities(),
        user_agent,
    )
}

/// Fails if user doesn’t exist, otherwise logs them in by creating a session.
pub async fn signin(
    State(state): State<AppState>,
    user_agent: Option<TypedHeader<UserAgent>>,
    cookies: Cookies,
    Host(host): Host,
    body: Bytes,
) -> HttpResult<impl IntoResponse> {
    // 1) Verify the AuthToken in the request body
    let token = state.verifier.verify(&body)?;
    let public_key = token.public_key();

    // 2) Ensure user *does* exist
    let txn = state.db.env.read_txn()?;
    let users = state.db.tables.users;
    let user_exists = users.get(&txn, public_key)?.is_some();
    txn.commit()?;
    if !user_exists {
        return Err(HttpError::new_with_message(
            StatusCode::NOT_FOUND,
            "User does not exist",
        ));
    }

    // 3) Create the session & set cookie
    create_session_and_cookie(
        &state,
        cookies,
        &host,
        public_key,
        token.capabilities(),
        user_agent,
    )
}

/// Creates and stores a session, sets the cookie, returns session as JSON/string.
fn create_session_and_cookie(
    state: &AppState,
    cookies: Cookies,
    host: &str,
    public_key: &PublicKey,
    capabilities: &Capabilities,
    user_agent: Option<TypedHeader<UserAgent>>,
) -> HttpResult<impl IntoResponse> {
    err_if_user_is_invalid(public_key, &state.db, false)?;

    // 1) Create session
    let session_secret = encode(Alphabet::Crockford, &random_bytes::<16>());
    let session = SessionInfo::new(
        public_key,
        capabilities.clone(),
        user_agent.map(|ua| ua.to_string()),
    )
    .serialize();

    // 2) Insert session into DB
    let mut wtxn = state.db.env.write_txn()?;
    state
        .db
        .tables
        .sessions
        .put(&mut wtxn, &session_secret, &session)?;
    wtxn.commit()?;

    // 3) Build and set cookie
    let mut cookie = Cookie::new(public_key.to_string(), session_secret);
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

    Ok(session)
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
