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
    /// Updates a user's data usage by a signed `delta` (bytes).
    /// Increases or decreases the stored `used_bytes` count for `public_key`.
    /// Negative results are clamped to zero to prevent underflow.
    pub fn update_data_usage(&mut self, public_key: &PublicKey, delta: i64) -> anyhow::Result<()> {
        let mut wtxn = self.env.write_txn()?;
        let mut user = self
            .tables
            .users
            .get(&wtxn, public_key)?
            .ok_or_else(|| anyhow::anyhow!("no user found for public key {:?}", public_key))?;

        if delta >= 0 {
            user.used_bytes = user.used_bytes.saturating_add(delta as u64);
        } else {
            user.used_bytes = user.used_bytes.saturating_sub((-delta) as u64);
        }

        self.tables.users.put(&mut wtxn, public_key, &user)?;
        wtxn.commit()?;
        Ok(())
    }

    /// Retrieves the current data usage (in bytes) for a given user.
    /// Returns the `used_bytes` value for the specified `public_key`, or zero if no record exists.
    pub fn get_user_data_usage(&self, pk: &PublicKey) -> anyhow::Result<u64> {
        let rtxn = self.env.read_txn()?;
        let usage = self
            .tables
            .users
            .get(&rtxn, pk)?
            .map(|u| u.used_bytes)
            .unwrap_or(0);
        rtxn.commit()?;
        Ok(usage)
    }

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
    #[cfg(test)]
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

#[cfg(test)]
mod unit_tests {
    use crate::persistence::lmdb::LmDB;
    use pkarr::Keypair;

    #[test]
    fn test_update_and_get_usage() {
        let mut db = LmDB::test();
        let key = Keypair::random().public_key();

        // create user
        let mut wtxn = db.env.write_txn().unwrap();
        db.create_user(&key, &mut wtxn).unwrap();
        wtxn.commit().unwrap();

        // initially zero
        assert_eq!(db.get_user_data_usage(&key).unwrap(), 0);
        db.update_data_usage(&key, 500).unwrap();
        assert_eq!(db.get_user_data_usage(&key).unwrap(), 500);
        // clamp at zero
        db.update_data_usage(&key, -600).unwrap();
        assert_eq!(db.get_user_data_usage(&key).unwrap(), 0);
    }
}
