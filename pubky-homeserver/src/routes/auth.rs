use axum::{
    extract::{Host, State},
    http::StatusCode,
    response::IntoResponse,
};
use axum_extra::{headers::UserAgent, TypedHeader};
use bytes::Bytes;
use tower_cookies::{cookie::SameSite, Cookie, Cookies};

use pubky_common::{crypto::random_bytes, session::Session, timestamp::Timestamp};

use crate::{
    database::tables::{
        sessions::{SessionsTable, SESSIONS_TABLE},
        users::User,
    },
    error::{Error, Result},
    extractors::Pubky,
    server::AppState,
};

pub async fn signup(
    State(state): State<AppState>,
    user_agent: Option<TypedHeader<UserAgent>>,
    cookies: Cookies,
    host: Host,
    body: Bytes,
) -> Result<impl IntoResponse> {
    // TODO: Verify invitation link.
    // TODO: add errors in case of already axisting user.
    signin(State(state), user_agent, cookies, host, body).await
}

pub async fn session(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: Pubky,
) -> Result<impl IntoResponse> {
    if let Some(cookie) = cookies.get(&pubky.public_key().to_string()) {
        let rtxn = state.db.env.read_txn()?;

        let sessions: SessionsTable = state
            .db
            .env
            .open_database(&rtxn, Some(SESSIONS_TABLE))?
            .expect("Session table already created");

        if let Some(session) = sessions.get(&rtxn, cookie.value())? {
            let session = session.to_owned();
            rtxn.commit()?;

            // TODO: add content-type
            return Ok(session);
        };

        rtxn.commit()?;
    };

    Err(Error::with_status(StatusCode::NOT_FOUND))
}

pub async fn signout(
    State(state): State<AppState>,
    cookies: Cookies,
    pubky: Pubky,
) -> Result<impl IntoResponse> {
    if let Some(cookie) = cookies.get(&pubky.public_key().to_string()) {
        let mut wtxn = state.db.env.write_txn()?;

        let sessions: SessionsTable = state
            .db
            .env
            .open_database(&wtxn, Some(SESSIONS_TABLE))?
            .expect("Session table already created");

        let _ = sessions.delete(&mut wtxn, cookie.value());

        wtxn.commit()?;

        return Ok(());
    };

    Err(Error::with_status(StatusCode::UNAUTHORIZED))
}

pub async fn signin(
    State(state): State<AppState>,
    user_agent: Option<TypedHeader<UserAgent>>,
    cookies: Cookies,
    Host(host): Host,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let token = state.verifier.verify(&body)?;

    let public_key = token.pubky();

    let mut wtxn = state.db.env.write_txn()?;

    let users = state.db.tables.users;
    if let Some(existing) = users.get(&wtxn, public_key)? {
        users.put(&mut wtxn, public_key, &existing)?;
    } else {
        users.put(
            &mut wtxn,
            public_key,
            &User {
                created_at: Timestamp::now().as_u64(),
            },
        )?;
    }

    let session_secret = base32::encode(base32::Alphabet::Crockford, &random_bytes::<16>());

    let session = Session::new(&token, user_agent.map(|ua| ua.to_string())).serialize();

    state
        .db
        .tables
        .sessions
        .put(&mut wtxn, &session_secret, &session)?;

    let mut cookie = Cookie::new(public_key.to_string(), session_secret);

    cookie.set_path("/");

    if is_secure(&host) {
        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
    }
    cookie.set_http_only(true);

    cookies.add(cookie);

    wtxn.commit()?;

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
