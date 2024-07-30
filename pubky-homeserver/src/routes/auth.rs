use axum::{
    debug_handler,
    extract::{Request, State},
    http::{uri::Scheme, HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    Router,
};
use axum_extra::{headers::UserAgent, TypedHeader};
use bytes::Bytes;
use heed::BytesEncode;
use postcard::to_allocvec;
use tower_cookies::{cookie::SameSite, Cookie, Cookies};

use pubky_common::{
    crypto::{random_bytes, random_hash},
    session::Session,
    timestamp::Timestamp,
};

use crate::{
    database::tables::{
        sessions::{SessionsTable, SESSIONS_TABLE},
        users::{User, UsersTable, USERS_TABLE},
    },
    error::{Error, Result},
    extractors::Pubky,
    server::AppState,
};

#[debug_handler]
pub async fn signup(
    State(state): State<AppState>,
    user_agent: Option<TypedHeader<UserAgent>>,
    cookies: Cookies,
    pubky: Pubky,
    uri: Uri,
    body: Bytes,
) -> Result<impl IntoResponse> {
    // TODO: Verify invitation link.
    // TODO: add errors in case of already axisting user.
    signin(State(state), user_agent, cookies, pubky, uri, body).await
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
    pubky: Pubky,
    uri: Uri,
    body: Bytes,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();

    state.verifier.verify(&body, public_key)?;

    let mut wtxn = state.db.env.write_txn()?;
    let users: UsersTable = state
        .db
        .env
        .open_database(&wtxn, Some(USERS_TABLE))?
        .expect("Users table already created");

    if let Some(existing) = users.get(&wtxn, public_key)? {
        users.put(&mut wtxn, public_key, &existing)?;
    } else {
        users.put(
            &mut wtxn,
            public_key,
            &User {
                created_at: Timestamp::now().into_inner(),
            },
        )?;
    }

    let session_secret = base32::encode(base32::Alphabet::Crockford, &random_bytes::<16>());

    let sessions: SessionsTable = state
        .db
        .env
        .open_database(&wtxn, Some(SESSIONS_TABLE))?
        .expect("Sessions table already created");

    let mut session = Session::new();

    if let Some(user_agent) = user_agent {
        session.set_user_agent(user_agent.to_string());
    }

    sessions.put(&mut wtxn, &session_secret, &session.serialize())?;

    let mut cookie = Cookie::new(public_key.to_string(), session_secret);
    cookie.set_path("/");
    if *uri.scheme().unwrap_or(&Scheme::HTTP) == Scheme::HTTPS {
        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
    }
    cookie.set_http_only(true);

    cookies.add(cookie);

    wtxn.commit()?;

    Ok(())
}
