pub mod blobs;
pub mod entries;
pub mod events;
pub mod sessions;
pub mod users;

use heed::{Env, RwTxn};

use blobs::{BlobsTable, BLOBS_TABLE};
use entries::{EntriesTable, ENTRIES_TABLE};

use self::{
    events::{EventsTable, EVENTS_TABLE},
    sessions::{SessionsTable, SESSIONS_TABLE},
    users::{UsersTable, USERS_TABLE},
};

pub const TABLES_COUNT: u32 = 5;

#[derive(Debug, Clone)]
pub struct Tables {
    pub users: UsersTable,
    pub sessions: SessionsTable,
    pub blobs: BlobsTable,
    pub entries: EntriesTable,
    pub events: EventsTable,
}

impl Tables {
    pub fn new(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<Self> {
        Ok(Self {
            users: env
                .open_database(wtxn, Some(USERS_TABLE))?
                .expect("Users table already created"),
            sessions: env
                .open_database(wtxn, Some(SESSIONS_TABLE))?
                .expect("Sessions table already created"),
            blobs: env
                .open_database(wtxn, Some(BLOBS_TABLE))?
                .expect("Blobs table already created"),
            entries: env
                .open_database(wtxn, Some(ENTRIES_TABLE))?
                .expect("Entries table already created"),
            events: env
                .open_database(wtxn, Some(EVENTS_TABLE))?
                .expect("Events table already created"),
        })
    }
}
