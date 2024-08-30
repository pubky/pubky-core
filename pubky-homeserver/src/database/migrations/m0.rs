use heed::{Env, RwTxn};

use crate::database::tables::{blobs, entries, events, sessions, users};

pub fn run(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    let _: users::UsersTable = env.create_database(wtxn, Some(users::USERS_TABLE))?;

    let _: sessions::SessionsTable = env.create_database(wtxn, Some(sessions::SESSIONS_TABLE))?;

    let _: blobs::BlobsTable = env.create_database(wtxn, Some(blobs::BLOBS_TABLE))?;

    let _: entries::EntriesTable = env.create_database(wtxn, Some(entries::ENTRIES_TABLE))?;

    let _: events::EventsTable = env.create_database(wtxn, Some(events::EVENTS_TABLE))?;

    Ok(())
}
