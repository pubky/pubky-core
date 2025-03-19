use heed::{Env, RwTxn};

use crate::core::database::tables::{blobs, entries, events, sessions, signup_tokens, users};

pub fn run(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    let _: users::UsersTable = env.create_database(wtxn, Some(users::USERS_TABLE))?;

    let _: sessions::SessionsTable = env.create_database(wtxn, Some(sessions::SESSIONS_TABLE))?;

    let _: blobs::BlobsTable = env.create_database(wtxn, Some(blobs::BLOBS_TABLE))?;

    let _: entries::EntriesTable = env.create_database(wtxn, Some(entries::ENTRIES_TABLE))?;

    let _: events::EventsTable = env.create_database(wtxn, Some(events::EVENTS_TABLE))?;

    let _: signup_tokens::SignupTokensTable =
        env.create_database(wtxn, Some(signup_tokens::SIGNUP_TOKENS_TABLE))?;

    Ok(())
}
