//! Pubky homeserver session struct.

use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

extern crate alloc;
use alloc::vec::Vec;

use crate::{capabilities::Capability, timestamp::Timestamp};

// TODO: add IP address?
// TODO: use https://crates.io/crates/user-agent-parser to parse the session
// and get more informations from the user-agent.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
/// Pubky homeserver session struct.
pub struct Session {
    version: usize,
    public_key: PublicKey,
    created_at: u64,
    /// User specified name, defaults to the user-agent.
    name: String,
    user_agent: String,
    capabilities: Vec<Capability>,
}

impl Session {
    /// Create a new session.
    pub fn new(
        public_key: &PublicKey,
        capabilities: &[Capability],
        user_agent: Option<String>,
    ) -> Self {
        Self {
            version: 0,
            public_key: public_key.clone(),
            created_at: Timestamp::now().as_u64(),
            capabilities: capabilities.to_vec(),
            user_agent: user_agent.as_deref().unwrap_or("").to_string(),
            name: user_agent.as_deref().unwrap_or("").to_string(),
        }
    }

    // === Getters ===

    /// Returns the public_key of this session authorizes for.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Returns the capabilities this session provide on this session's pubky's resources.
    pub fn capabilities(&self) -> &Vec<Capability> {
        &self.capabilities
    }

    // === Setters ===

    /// Set this session user agent.
    pub fn set_user_agent(&mut self, user_agent: String) -> &mut Self {
        self.user_agent = user_agent;

        if self.name.is_empty() {
            self.name.clone_from(&self.user_agent)
        }

        self
    }

    /// Set this session's capabilities.
    pub fn set_capabilities(&mut self, capabilities: Vec<Capability>) -> &mut Self {
        self.capabilities = capabilities;

        self
    }

    // === Public Methods ===

    /// Serialize this session to its canonical binary representation.
    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("Session::serialize")
    }

    /// Deserialize this session from its canonical binary representation.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() {
            return Err(Error::EmptyPayload);
        }

        if bytes[0] > 0 {
            return Err(Error::UnknownVersion);
        }

        Ok(from_bytes(bytes)?)
    }

    // TODO: add `can_read()`, `can_write()` and `is_root()` methods
}

#[derive(thiserror::Error, Debug, PartialEq)]
/// Error deserializing a [Session].
pub enum Error {
    #[error("Empty payload")]
    /// Empty payload
    EmptyPayload,
    #[error("Unknown version")]
    /// Unknown version
    UnknownVersion,
    #[error(transparent)]
    /// Error parsing the binary representation.
    Parsing(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::*;

    #[test]
    fn serialize() {
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let public_key = keypair.public_key();

        let session = Session {
            user_agent: "foo".to_string(),
            capabilities: vec![Capability::root()],
            created_at: 0,
            public_key,
            version: 0,
            name: "".to_string(),
        };

        let serialized = session.serialize();

        assert_eq!(
            serialized,
            [
                0, 59, 106, 39, 188, 206, 182, 164, 45, 98, 163, 168, 208, 42, 111, 13, 115, 101,
                50, 21, 119, 29, 226, 67, 166, 58, 192, 72, 161, 139, 89, 218, 41, 0, 0, 3, 102,
                111, 111, 1, 4, 47, 58, 114, 119
            ]
        );

        let deseiralized = Session::deserialize(&serialized).unwrap();

        assert_eq!(deseiralized, session)
    }

    #[test]
    fn deserialize() {
        let result = Session::deserialize(&[]);

        assert_eq!(result, Err(Error::EmptyPayload));
    }
}
