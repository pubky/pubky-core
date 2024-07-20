use std::{borrow::Cow, time::SystemTime};

use postcard::{from_bytes, to_allocvec};
use pubky_common::timestamp::Timestamp;
use serde::{Deserialize, Serialize};

use heed::{types::Bytes, BoxedError, BytesDecode, BytesEncode, Database};
use pkarr::PublicKey;

extern crate alloc;
use alloc::vec::Vec;

/// session secret => Session.
pub type SessionsTable = Database<Bytes, Session>;

pub const SESSIONS_TABLE: &str = "sessions";

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Session {
    pub created_at: u64,
    pub name: String,
}

impl<'a> BytesEncode<'a> for Session {
    type EItem = Self;

    fn bytes_encode(session: &Self::EItem) -> Result<Cow<[u8]>, BoxedError> {
        let vec = to_allocvec(session)?;

        Ok(Cow::Owned(vec))
    }
}

impl<'a> BytesDecode<'a> for Session {
    type DItem = Self;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let sesison: Session = from_bytes(bytes).unwrap();

        Ok(sesison)
    }
}
