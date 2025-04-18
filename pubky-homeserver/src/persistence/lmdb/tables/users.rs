use std::borrow::Cow;

use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

use heed::{BoxedError, BytesDecode, BytesEncode, Database};
use pkarr::{PublicKey, Timestamp};

extern crate alloc;

/// PublicKey => User.
pub type UsersTable = Database<PublicKeyCodec, User>;

pub const USERS_TABLE: &str = "users";

// TODO: add more adminstration metadata like quota, invitation links, etc..
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct User {
    pub created_at: u64,
}

impl User {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            created_at: Timestamp::now().as_u64(),
        }
    }
}

impl BytesEncode<'_> for User {
    type EItem = Self;

    fn bytes_encode(user: &Self::EItem) -> Result<Cow<[u8]>, BoxedError> {
        let vec = to_allocvec(user).unwrap();

        Ok(Cow::Owned(vec))
    }
}

impl<'a> BytesDecode<'a> for User {
    type DItem = Self;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let user: User = from_bytes(bytes).unwrap();

        Ok(user)
    }
}

pub struct PublicKeyCodec {}

impl BytesEncode<'_> for PublicKeyCodec {
    type EItem = PublicKey;

    fn bytes_encode(pubky: &Self::EItem) -> Result<Cow<[u8]>, BoxedError> {
        Ok(Cow::Borrowed(pubky.as_bytes()))
    }
}

impl<'a> BytesDecode<'a> for PublicKeyCodec {
    type DItem = PublicKey;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        Ok(PublicKey::try_from(bytes)?)
    }
}
