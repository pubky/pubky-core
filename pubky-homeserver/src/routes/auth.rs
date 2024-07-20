use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use axum_extra::{headers::UserAgent, TypedHeader};
use bytes::Bytes;
use tower_cookies::{Cookie, Cookies};

use pubky_common::{
    crypto::{random_bytes, random_hash},
    timestamp::Timestamp,
};

use crate::{
    database::tables::{
        sessions::{Session, SessionsTable, SESSIONS_TABLE},
        users::{User, UsersTable, USERS_TABLE},
    },
    error::{Error, Result},
    extractors::Pubky,
    server::AppState,
};

pub async fn signup(
    State(state): State<AppState>,
    TypedHeader(user_agent): TypedHeader<UserAgent>,
    cookies: Cookies,
    pubky: Pubky,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();

    state.verifier.verify(&body, public_key)?;

    let mut wtxn = state.db.env.write_txn()?;
    let users: UsersTable = state.db.env.create_database(&mut wtxn, Some(USERS_TABLE))?;

    users.put(
        &mut wtxn,
        public_key,
        &User {
            created_at: Timestamp::now().into_inner(),
        },
    )?;

    let session_secret = random_bytes::<16>();

    let sessions: SessionsTable = state
        .db
        .env
        .open_database(&wtxn, Some(SESSIONS_TABLE))?
        .expect("Sessions table already created");

    // TODO: handle not having a user agent?
    let session = &Session {
        created_at: Timestamp::now().into_inner(),
        name: user_agent.to_string(),
    };

    sessions.put(&mut wtxn, &session_secret, session)?;

    cookies.add(Cookie::new(
        public_key.to_string(),
        base32::encode(base32::Alphabet::Crockford, &session_secret),
    ));

    wtxn.commit()?;

    Ok(())
}

pub async fn session(
    State(state): State<AppState>,
    TypedHeader(user_agent): TypedHeader<UserAgent>,
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

        if let Some(session) = sessions.get(
            &rtxn,
            &base32::decode(base32::Alphabet::Crockford, cookie.value()).unwrap_or_default(),
        )? {
            rtxn.commit()?;
            return Ok(());
        };

        rtxn.commit()?;
    };

    Err(Error::with_status(StatusCode::NOT_FOUND))
}
