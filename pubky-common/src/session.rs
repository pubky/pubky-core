use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

extern crate alloc;
use alloc::vec::Vec;

use crate::timestamp::Timestamp;

// TODO: add IP address?
// TODO: use https://crates.io/crates/user-agent-parser to parse the session
// and get more informations from the user-agent.
#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Session {
    pub version: usize,
    pub created_at: u64,
    /// User specified name, defaults to the user-agent.
    pub name: String,
    pub user_agent: String,
}

impl Session {
    pub fn new() -> Self {
        Self {
            created_at: Timestamp::now().into_inner(),
            ..Default::default()
        }
    }

    // === Setters ===

    pub fn set_user_agent(&mut self, user_agent: String) -> &mut Self {
        self.user_agent = user_agent;

        if self.name.is_empty() {
            self.name.clone_from(&self.user_agent)
        }

        self
    }

    // === Public Methods ===

    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("Session::serialize")
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes[0] > 0 {
            return Err(Error::UnknownVersion);
        }

        Ok(from_bytes(bytes)?)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unknown version")]
    UnknownVersion,
    #[error(transparent)]
    Postcard(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize() {
        let session = Session {
            user_agent: "foo".to_string(),
            ..Default::default()
        };

        let serialized = session.serialize();

        assert_eq!(serialized, [0, 0, 0, 3, 102, 111, 111,]);

        let deseiralized = Session::deserialize(&serialized).unwrap();

        assert_eq!(deseiralized, session)
    }
}
