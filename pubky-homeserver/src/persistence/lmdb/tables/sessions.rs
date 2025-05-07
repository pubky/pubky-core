use std::ops::Deref;

use heed::{
    types::{Bytes, Str},
    Database,
};
use pkarr::PublicKey;
use pubky_common::{capabilities::Capability, crypto::random_bytes, session::Session};

use super::super::LmDB;

/// session secret => Session.
pub type SessionsTable = Database<Str, Bytes>;

pub const SESSIONS_TABLE: &str = "sessions";

/// A session ID is a base32 encoded string.
/// Basically a named string.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SessionId(pub String);

impl Deref for SessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}


impl LmDB {
    /// Creates a new session in the database.
    pub fn create_session(
        &self,
        user_pubkey: &PublicKey,
        capabilities: &[Capability],
    ) -> anyhow::Result<(SessionId, Session)> {
        // 1) Create session
        let session_id = base32::encode(base32::Alphabet::Crockford, &random_bytes::<16>());
        let raw_session = Session::new(user_pubkey, capabilities, None).serialize();

        // 2) Insert session into DB
        let mut wtxn = self.env.write_txn()?;
        self
            .tables
            .sessions
            .put(&mut wtxn, &session_id, &raw_session)?;
        wtxn.commit()?;

        let session = Session::deserialize(&raw_session)?;
        Ok((SessionId(session_id), session))
    }

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
