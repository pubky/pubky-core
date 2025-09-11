use heed::{
    types::{Bytes, Str},
    Database,
};

/// session secret => Session.
pub type SessionsTable = Database<Str, Bytes>;

pub const SESSIONS_TABLE: &str = "sessions";
