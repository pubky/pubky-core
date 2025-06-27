pub mod events;
pub mod sessions;
pub mod signup_tokens;
pub mod users;
pub mod entries;
use heed::{Env, RwTxn};

use self::{
    events::{EventsTable, EVENTS_TABLE},
    sessions::{SessionsTable, SESSIONS_TABLE},
    signup_tokens::{SignupTokensTable, SIGNUP_TOKENS_TABLE},
    users::{UsersTable, USERS_TABLE},
    entries::{EntriesTable, ENTRIES_TABLE},
};

pub const TABLES_COUNT: u32 = 6;

#[derive(Debug, Clone)]
pub struct Tables {
    pub users: UsersTable,
    pub sessions: SessionsTable,
    pub entries: EntriesTable,
    pub events: EventsTable,
    pub signup_tokens: SignupTokensTable,
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
            entries: env
                .open_database(wtxn, Some(ENTRIES_TABLE))?
                .expect("Entries table already created"),
            events: env
                .open_database(wtxn, Some(EVENTS_TABLE))?
                .expect("Events table already created"),
            signup_tokens: env
                .open_database(wtxn, Some(SIGNUP_TOKENS_TABLE))?
                .expect("Signup tokens table already created"),
        })
    }
}
