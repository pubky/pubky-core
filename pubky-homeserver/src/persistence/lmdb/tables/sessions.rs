use std::ops::Deref;

use heed::{
    types::{Bytes, Str},
    Database,
};
use pkarr::PublicKey;
use pubky_common::{capabilities::Capability, crypto::random_bytes, session::Session};
use serde::{Deserialize, Serialize};

use super::super::LmDB;

/// session secret => Session.
pub type SessionsTable = Database<Str, Bytes>;

pub const SESSIONS_TABLE: &str = "sessions";

/// A session ID is a base32 encoded string with 32 characters.
/// This validates the session ID and adds support for (de)serialization.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub struct SessionId(pub String);

impl Deref for SessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let session_id = String::deserialize(deserializer)?;
        SessionId::new(&session_id).map_err(serde::de::Error::custom)
    }
}

impl SessionId {
    pub fn new(session_id: &str) -> anyhow::Result<Self> {
        if !Self::is_valid_id(session_id) {
            return Err(anyhow::anyhow!("Invalid session ID"));
        }

        Ok(Self(session_id.to_string()))
    }

    /// Checks if a session ID is valid.
    ///
    /// A session ID is valid if it is a base32 encoded string of 16 bytes.
    pub fn is_valid_id(session_id: &str) -> bool {
        match base32::decode(base32::Alphabet::Crockford, session_id) {
            Some(decoded) => decoded.len() == 16,
            None => false,
        }
    }

    pub fn random() -> Self {
        Self(base32::encode(
            base32::Alphabet::Crockford,
            &random_bytes::<16>(),
        ))
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
        let session_id = SessionId::random();
        let raw_session = Session::new(user_pubkey, capabilities, None).serialize();

        // 2) Insert session into DB
        let mut wtxn = self.env.write_txn()?;
        self.tables
            .sessions
            .put(&mut wtxn, &session_id, &raw_session)?;
        wtxn.commit()?;

        let session = Session::deserialize(&raw_session)?;
        Ok((session_id, session))
    }

    pub fn get_session(&self, session_id: &str) -> anyhow::Result<Option<Session>> {
        let rtxn = self.env.read_txn()?;

        let session = self
            .tables
            .sessions
            .get(&rtxn, session_id)?
            .map(|s| s.to_vec());

        rtxn.commit()?;

        if let Some(bytes) = session {
            return Ok(Some(Session::deserialize(&bytes)?));
        };

        Ok(None)
    }

    pub fn delete_session(&mut self, session_id: &str) -> anyhow::Result<bool> {
        let mut wtxn = self.env.write_txn()?;

        let deleted = self.tables.sessions.delete(&mut wtxn, session_id)?;

        wtxn.commit()?;

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_is_valid() {
        let session_id = SessionId::random();
        assert!(SessionId::is_valid_id(&session_id));
    }

    #[test]
    fn test_session_id_is_invalid() {
        let session_id = "invalid";
        assert!(!SessionId::is_valid_id(&session_id));
    }

    #[test]
    fn test_session_id_deserialize() {
        let session_id = SessionId::random();
        let serialized = serde_json::to_string(&session_id).unwrap();
        let deserialized: SessionId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(session_id, deserialized);
    }
}
