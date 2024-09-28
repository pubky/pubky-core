use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

extern crate alloc;
use alloc::vec::Vec;

use crate::{auth::AuthToken, capabilities::Capability, timestamp::Timestamp};

// TODO: add IP address?
// TODO: use https://crates.io/crates/user-agent-parser to parse the session
// and get more informations from the user-agent.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Session {
    version: usize,
    pubky: PublicKey,
    created_at: u64,
    /// User specified name, defaults to the user-agent.
    name: String,
    user_agent: String,
    capabilities: Vec<Capability>,
}

impl Session {
    pub fn new(token: &AuthToken, user_agent: Option<String>) -> Self {
        Self {
            version: 0,
            pubky: token.pubky().to_owned(),
            created_at: Timestamp::now().into_inner(),
            capabilities: token.capabilities().to_vec(),
            user_agent: user_agent.as_deref().unwrap_or("").to_string(),
            name: user_agent.as_deref().unwrap_or("").to_string(),
        }
    }

    // === Getters ===

    pub fn pubky(&self) -> &PublicKey {
        &self.pubky
    }

    pub fn capabilities(&self) -> &Vec<Capability> {
        &self.capabilities
    }

    // === Setters ===

    pub fn set_user_agent(&mut self, user_agent: String) -> &mut Self {
        self.user_agent = user_agent;

        if self.name.is_empty() {
            self.name.clone_from(&self.user_agent)
        }

        self
    }

    pub fn set_capabilities(&mut self, capabilities: Vec<Capability>) -> &mut Self {
        self.capabilities = capabilities;

        self
    }

    // === Public Methods ===

    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("Session::serialize")
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
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

pub type Result<T> = core::result::Result<T, Error>;

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum Error {
    #[error("Empty payload")]
    EmptyPayload,
    #[error("Unknown version")]
    UnknownVersion,
    #[error(transparent)]
    Postcard(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::*;

    #[test]
    fn serialize() {
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubky = keypair.public_key();

        let session = Session {
            user_agent: "foo".to_string(),
            capabilities: vec![Capability::root()],
            created_at: 0,
            pubky,
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
