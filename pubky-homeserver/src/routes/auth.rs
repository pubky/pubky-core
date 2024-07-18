use axum::{extract::State, response::IntoResponse};
use bytes::Bytes;

use pubky_common::timestamp::Timestamp;

use crate::{
    database::tables::users::{User, UsersTable, USERS_TABLE},
    error::Result,
    extractors::Pubky,
    server::AppState,
};

pub async fn signup(
    State(state): State<AppState>,
    pubky: Pubky,
    body: Bytes,
) -> Result<impl IntoResponse> {
    state.verifier.verify(&body, pubky.public_key())?;

    let mut wtxn = state.db.env.write_txn()?;
    let users: UsersTable = state.db.env.create_database(&mut wtxn, Some(USERS_TABLE))?;

    users.put(
        &mut wtxn,
        pubky.public_key(),
        &User {
            created_at: Timestamp::now().into_inner(),
        },
    )?;

    Ok(())
}
