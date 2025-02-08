use heed::{
    types::{Bytes, Str},
    Database,
};
use pubky_common::session::Session;

use crate::core::database::DB;

/// session secret => Session.
pub type SessionsTable = Database<Str, Bytes>;

pub const SESSIONS_TABLE: &str = "sessions";

impl DB {
    pub fn get_session(&self, session_secret: &str) -> anyhow::Result<Option<Session>> {
        let rtxn = self.env.read_txn()?;

        let session = self
            .tables
            .sessions
            .get(&rtxn, session_secret)?
            .map(|s| s.to_vec());

        rtxn.commit()?;

        if let Some(bytes) = session {
            return Ok(Some(Session::deserialize(&bytes)?));
        };

        Ok(None)
    }

    pub fn delete_session(&mut self, secret: &str) -> anyhow::Result<bool> {
        let mut wtxn = self.env.write_txn()?;

        let deleted = self.tables.sessions.delete(&mut wtxn, secret)?;

        wtxn.commit()?;

        Ok(deleted)
    }
}
