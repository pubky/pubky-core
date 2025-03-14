use crate::core::{
    database::tables::users::User,
    error::{Error, Result},
    AppState, SignupMode,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::{extract::Host, headers::UserAgent, TypedHeader};
use base32::{encode, Alphabet};
use bytes::Bytes;
use pkarr::PublicKey;
use pubky_common::{
    capabilities::Capability, crypto::random_bytes, session::Session, timestamp::Timestamp,
};
use std::collections::HashMap;
use tower_cookies::{cookie::SameSite, Cookie, Cookies};

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
) -> Result<impl IntoResponse> {
    // 1) Verify AuthToken from request body
    let token = state.verifier.verify(&body)?;
    let public_key = token.pubky();

    // 2) Ensure the user does *not* already exist
    let txn = state.db.env.read_txn()?;
    let users = state.db.tables.users;
    if users.get(&txn, public_key)?.is_some() {
        return Err(Error::new(
            StatusCode::CONFLICT,
            Some("User already exists"),
        ));
    }
    txn.commit()?;

    // 3) If signup_mode == closed, require & validate a `signup_token` param.
    if state.admin.signup_mode == SignupMode::TokenRequired {
        let signup_token_param = params
            .get("signup_token")
            .ok_or_else(|| Error::new(StatusCode::BAD_REQUEST, Some("signup_token required")))?;
        // Validate it in the DB (marks it used)
        state
            .db
            .validate_and_consume_signup_token(signup_token_param, public_key)?;
    }

    // 4) Create the new user record
    let mut wtxn = state.db.env.write_txn()?;
    users.put(
        &mut wtxn,
        public_key,
        &User {
            created_at: Timestamp::now().as_u64(),
        },
    )?;
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
) -> Result<impl IntoResponse> {
    // 1) Verify the AuthToken in the request body
    let token = state.verifier.verify(&body)?;
    let public_key = token.pubky();

    // 2) Ensure user *does* exist
    let txn = state.db.env.read_txn()?;
    let users = state.db.tables.users;
    let user_exists = users.get(&txn, public_key)?.is_some();
    txn.commit()?;
    if !user_exists {
        return Err(Error::new(
            StatusCode::NOT_FOUND,
            Some("User does not exist"),
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
    capabilities: &[Capability],
    user_agent: Option<TypedHeader<UserAgent>>,
) -> Result<impl IntoResponse> {
    // 1) Create session
    let session_secret = encode(Alphabet::Crockford, &random_bytes::<16>());
    let session = Session::new(
        public_key,
        capabilities,
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
        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
    }
    cookie.set_http_only(true);
    cookies.add(cookie);

    Ok(session)
}

/// Assuming that if the server is addressed by anything other than
/// localhost, or IP addresses, it is not addressed from a browser in an
/// secure (HTTPs) window, thus it no need to `secure` and `same_site=none` to cookies
fn is_secure(host: &str) -> bool {
    url::Host::parse(host)
        .map(|host| match host {
            url::Host::Domain(domain) => domain != "localhost",
            _ => false,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use super::*;

    #[test]
    fn test_is_secure() {
        assert!(!is_secure(""));
        assert!(!is_secure("127.0.0.1"));
        assert!(!is_secure("167.86.102.121"));
        assert!(!is_secure("[2001:0db8:0000:0000:0000:ff00:0042:8329]"));
        assert!(!is_secure("localhost"));
        assert!(!is_secure("localhost:23423"));
        assert!(is_secure(&Keypair::random().public_key().to_string()));
        assert!(is_secure("example.com"));
    }
}
