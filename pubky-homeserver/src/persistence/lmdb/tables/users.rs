use std::borrow::Cow;

use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

use heed::{BoxedError, BytesDecode, BytesEncode, Database, RoTxn, RwTxn};
use pkarr::{PublicKey, Timestamp};

use crate::persistence::lmdb::LmDB;

extern crate alloc;

/// PublicKey => User.
pub type UsersTable = Database<PublicKeyCodec, User>;

pub const USERS_TABLE: &str = "users";

// TODO: add more adminstration metadata like quota, invitation links, etc..
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct User {
    pub created_at: u64,
    pub disabled: bool,
    pub used_bytes: u64,
}

impl Default for User {
    fn default() -> Self {
        Self {
            created_at: Timestamp::now().as_u64(),
            disabled: false,
            used_bytes: 0,
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

#[derive(Debug, thiserror::Error)]
pub enum UserQueryError {
    #[error("User not found")]
    UserNotFound,
    #[error("transparent")]
    DatabaseError(#[from] heed::Error),
}

impl LmDB {
    /// Disable a user.
    ///
    /// # Errors
    ///
    /// - `UserQueryError::UserNotFound` if the user does not exist.
    /// - `UserQueryError::DatabaseError` if the database operation fails.
    pub fn disable_user(&self, pubkey: &PublicKey, wtxn: &mut RwTxn) -> Result<(), UserQueryError> {
        let mut user = match self.tables.users.get(wtxn, pubkey)? {
            Some(user) => user,
            None => return Err(UserQueryError::UserNotFound),
        };

        user.disabled = true;
        self.tables.users.put(wtxn, pubkey, &user)?;

        Ok(())
    }

    /// Enable a user.
    ///
    /// # Errors
    ///
    /// - `UserQueryError::UserNotFound` if the user does not exist.
    /// - `UserQueryError::DatabaseError` if the database operation fails.
    pub fn enable_user(&self, pubkey: &PublicKey, wtxn: &mut RwTxn) -> Result<(), UserQueryError> {
        let mut user = match self.tables.users.get(wtxn, pubkey)? {
            Some(user) => user,
            None => return Err(UserQueryError::UserNotFound),
        };

        user.disabled = false;
        self.tables.users.put(wtxn, pubkey, &user)?;

        Ok(())
    }

    /// Create a user.
    ///
    /// # Errors
    ///
    /// - `UserQueryError::DatabaseError` if the database operation fails.
    pub fn create_user(&self, pubkey: &PublicKey, wtxn: &mut RwTxn) -> anyhow::Result<()> {
        let user = User::default();
        self.tables.users.put(wtxn, pubkey, &user)?;
        Ok(())
    }

    /// Get a user.
    ///
    /// # Errors
    ///
    /// - `UserQueryError::UserNotFound` if the user does not exist.
    /// - `UserQueryError::DatabaseError` if the database operation fails.
    pub fn get_user(&self, pubkey: &PublicKey, wtxn: &mut RoTxn) -> Result<User, UserQueryError> {
        let user = match self.tables.users.get(wtxn, pubkey)? {
            Some(user) => user,
            None => return Err(UserQueryError::UserNotFound),
        };
        Ok(user)
    }
}
