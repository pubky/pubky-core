//! Pubky homeserver session struct.

use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

extern crate alloc;
use alloc::vec::Vec;

use crate::{
    capabilities::{Capabilities, Capability},
    timestamp::Timestamp,
};

// TODO: add IP address?
// TODO: use https://crates.io/crates/user-agent-parser to parse the session
// and get more informations from the user-session.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
/// Pubky homeserver session struct.
pub struct SessionInfo {
    version: usize,
    pubky: PublicKey,
    created_at: u64,
    /// User specified name, defaults to the user-session.
    name: String,
    user_agent: String,
    capabilities: Vec<Capability>,
}

impl SessionInfo {
    /// Create a new session.
    pub fn new(
        public_key: &PublicKey,
        capabilities: Capabilities,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            version: 0,
            pubky: public_key.clone(),
            created_at: Timestamp::now().as_u64(),
            capabilities: capabilities.to_vec(),
            user_agent: user_agent.as_deref().unwrap_or("").to_string(),
            name: user_agent.as_deref().unwrap_or("").to_string(),
        }
    }

    // === Getters ===

    /// Returns the public_key of this session authorizes for.
    pub fn public_key(&self) -> &PublicKey {
        &self.pubky
    }

    /// Returns the capabilities this session provide on this session's pubky's resources.
    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    // === Setters ===

    /// Set this session user session.
    pub fn set_user_agent(&mut self, user_agent: String) -> &mut Self {
        self.user_agent = user_agent;

        if self.name.is_empty() {
            self.name.clone_from(&self.user_agent)
        }

        self
    }

    /// Set this session's capabilities.
    pub fn set_capabilities(&mut self, capabilities: Capabilities) -> &mut Self {
        self.capabilities = capabilities.to_vec();

        self
    }

    // === Public Methods ===

    /// Serialize this session to its canonical binary representation.
    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("SessionInfo::serialize")
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
/// Error deserializing a [SessionInfo].
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
    use crate::{capabilities::Capability, crypto::Keypair};

    use super::*;

    #[test]
    fn serialize() {
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubky = keypair.public_key();
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();

        let session = SessionInfo {
            user_agent: "foo".to_string(),
            capabilities: capabilities.to_vec(),
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

        let deserialized = SessionInfo::deserialize(&serialized).unwrap();

        assert_eq!(deserialized, session)
    }

    #[test]
    fn deserialize() {
        let result = SessionInfo::deserialize(&[]);

        assert_eq!(result, Err(Error::EmptyPayload));
    }
}
