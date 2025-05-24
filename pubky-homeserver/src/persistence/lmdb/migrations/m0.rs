use heed::{Env, RwTxn};

use crate::persistence::lmdb::tables::{events, files, sessions, signup_tokens, users};

pub fn run(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    let _: users::UsersTable = env.create_database(wtxn, Some(users::USERS_TABLE))?;

    let _: sessions::SessionsTable = env.create_database(wtxn, Some(sessions::SESSIONS_TABLE))?;

    let _: files::BlobsTable = env.create_database(wtxn, Some(files::BLOBS_TABLE))?;

    let _: files::EntriesTable = env.create_database(wtxn, Some(files::ENTRIES_TABLE))?;

    let _: events::EventsTable = env.create_database(wtxn, Some(events::EVENTS_TABLE))?;

    let _: signup_tokens::SignupTokensTable =
        env.create_database(wtxn, Some(signup_tokens::SIGNUP_TOKENS_TABLE))?;

    Ok(())
}
