use axum::{
    debug_handler,
    extract::State,
    http::{uri::Scheme, StatusCode, Uri},
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

#[debug_handler]
pub async fn signup(
    State(state): State<AppState>,
    user_agent: Option<TypedHeader<UserAgent>>,
    cookies: Cookies,
    uri: Uri,
    body: Bytes,
) -> Result<impl IntoResponse> {
    // TODO: Verify invitation link.
    // TODO: add errors in case of already axisting user.
    signin(State(state), user_agent, cookies, uri, body).await
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
    uri: Uri,
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
                created_at: Timestamp::now().into_inner(),
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
    if *uri.scheme().unwrap_or(&Scheme::HTTP) == Scheme::HTTPS {
        cookie.set_secure(true);
        cookie.set_same_site(SameSite::None);
    }
    cookie.set_http_only(true);

    cookies.add(cookie);

    wtxn.commit()?;

    Ok(session)
}
