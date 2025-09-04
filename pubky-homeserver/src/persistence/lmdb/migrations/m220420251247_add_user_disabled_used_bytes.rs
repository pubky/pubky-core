use super::super::tables::users;
use crate::persistence::lmdb::tables::users::PublicKeyCodec;
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, RwTxn};
use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Adds the `disabled` field to the `users` table.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct OldUser {
    pub created_at: u64,
}

impl BytesEncode<'_> for OldUser {
    type EItem = Self;

    fn bytes_encode(user: &Self::EItem) -> Result<Cow<'_, [u8]>, BoxedError> {
        let vec = to_allocvec(user).unwrap();

        Ok(Cow::Owned(vec))
    }
}

impl<'a> BytesDecode<'a> for OldUser {
    type DItem = Self;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let user: OldUser = from_bytes(bytes).unwrap();

        Ok(user)
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct NewUser {
    pub created_at: u64,
    pub disabled: bool,
    pub used_bytes: u64,
}

impl BytesEncode<'_> for NewUser {
    type EItem = Self;

    fn bytes_encode(user: &Self::EItem) -> Result<Cow<'_, [u8]>, BoxedError> {
        let vec = to_allocvec(user)?;

        Ok(Cow::Owned(vec))
    }
}

impl From<OldUser> for NewUser {
    fn from(user: OldUser) -> Self {
        Self {
            created_at: user.created_at,
            disabled: false,
            used_bytes: 0,
        }
    }
}

impl<'a> BytesDecode<'a> for NewUser {
    type DItem = Self;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let user: NewUser = from_bytes(bytes)?;

        Ok(user)
    }
}

/// Checks if the migration is needed.
/// Tries to read users with the new schema. If it succeeds, the migration is not needed.
/// If it fails, the migration is needed.
fn is_migration_needed(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<bool> {
    let new_table: Database<PublicKeyCodec, NewUser> = env
        .open_database(wtxn, Some(users::USERS_TABLE))?
        .expect("User database is not available");

    match new_table.first(wtxn) {
        Ok(Some(_user)) => {
            // User found. The new schema is valid.
            // Migrations has already been run.
            Ok(false)
        }
        Ok(None) => {
            // No users found. No need to run the migration.
            Ok(false)
        }
        Err(_e) => {
            // Failed to deserialize. It's the old schema.
            // Migrations is needed.
            Ok(true)
        }
    }
}

fn read_old_users_table(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<Vec<(PublicKey, OldUser)>> {
    let table: Database<PublicKeyCodec, OldUser> = env
        .open_database(wtxn, Some(users::USERS_TABLE))?
        .expect("User database is not available");

    let mut new_users: Vec<(PublicKey, OldUser)> = vec![];
    for entry in table.iter(wtxn)? {
        let (key, old_user) = entry?;
        new_users.push((key, old_user));
    }

    Ok(new_users)
}

fn write_new_users_table(
    env: &Env,
    wtxn: &mut RwTxn,
    users: Vec<(PublicKey, NewUser)>,
) -> anyhow::Result<()> {
    let table: Database<PublicKeyCodec, NewUser> = env
        .open_database(wtxn, Some(users::USERS_TABLE))?
        .expect("User database is not available");

    for (key, new_user) in users {
        table.put(wtxn, &key, &new_user)?;
    }

    Ok(())
}

pub fn run(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    if !is_migration_needed(env, wtxn)? {
        return Ok(());
    }

    tracing::info!("Running migration 220420251247_add_user_disabled");
    let old_users = read_old_users_table(env, wtxn)
        .map_err(|e| anyhow::anyhow!("Failed to read old users table: {}", e))?;

    // Migrate the users to the new schema.
    let new_users: Vec<(PublicKey, NewUser)> = old_users
        .into_iter()
        .map(|(key, old_user)| (key, old_user.into()))
        .collect();

    tracing::info!("Read {} users", new_users.len());
    write_new_users_table(env, wtxn, new_users)
        .map_err(|e| anyhow::anyhow!("Failed to write new users table: {}", e))?;

    tracing::info!("Successfully migrated");

    Ok(())
}

#[cfg(test)]
mod tests {
    use heed::EnvOpenOptions;
    use pkarr::Keypair;

    use crate::persistence::lmdb::{db::DEFAULT_MAP_SIZE, migrations::m0};

    use super::*;

    #[test]
    fn test_is_migration_needed_yes() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(20)
                .map_size(DEFAULT_MAP_SIZE)
                .open(tmp_dir.path())
        }
        .unwrap();
        m0::run(&env, &mut env.write_txn().unwrap()).unwrap();
        let mut wtxn = env.write_txn().unwrap();

        // Write a user to the old table.
        let table: Database<PublicKeyCodec, OldUser> = env
            .create_database(&mut wtxn, Some(users::USERS_TABLE))
            .unwrap();
        table
            .put(
                &mut wtxn,
                &Keypair::random().public_key(),
                &OldUser { created_at: 1 },
            )
            .unwrap();

        assert!(is_migration_needed(&env, &mut wtxn).unwrap());
    }

    #[test]
    fn test_is_migration_needed_no_users() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(20)
                .map_size(DEFAULT_MAP_SIZE)
                .open(tmp_dir.path())
        }
        .unwrap();
        m0::run(&env, &mut env.write_txn().unwrap()).unwrap();
        let mut wtxn = env.write_txn().unwrap();

        // Write a user to the old table.
        let _: Database<PublicKeyCodec, OldUser> = env
            .create_database(&mut wtxn, Some(users::USERS_TABLE))
            .unwrap();

        assert!(!is_migration_needed(&env, &mut wtxn).unwrap());
    }

    #[test]
    fn test_is_migration_needed_already_migrated() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(20)
                .map_size(DEFAULT_MAP_SIZE)
                .open(tmp_dir.path())
        }
        .unwrap();
        m0::run(&env, &mut env.write_txn().unwrap()).unwrap();
        let mut wtxn = env.write_txn().unwrap();

        // Write a user to the new table.
        let table: Database<PublicKeyCodec, NewUser> = env
            .create_database(&mut wtxn, Some(users::USERS_TABLE))
            .unwrap();
        table
            .put(
                &mut wtxn,
                &Keypair::random().public_key(),
                &NewUser {
                    created_at: 1,
                    disabled: false,
                    used_bytes: 0,
                },
            )
            .unwrap();

        assert!(
            !is_migration_needed(&env, &mut wtxn).unwrap(),
            "The migration should not be needed anymore because it's already been run."
        );
    }

    #[test]
    fn test_migrate() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(20)
                .map_size(DEFAULT_MAP_SIZE)
                .open(tmp_dir.path())
        }
        .unwrap();
        m0::run(&env, &mut env.write_txn().unwrap()).unwrap();
        let mut wtxn = env.write_txn().unwrap();

        // Write a user to the old table.
        let pubkey = Keypair::random().public_key();
        let table: Database<PublicKeyCodec, OldUser> = env
            .create_database(&mut wtxn, Some(users::USERS_TABLE))
            .unwrap();
        table
            .put(&mut wtxn, &pubkey, &OldUser { created_at: 1 })
            .unwrap();

        // Migrate the users to the new schema.
        run(&env, &mut wtxn).unwrap();

        // Check that the user has been migrated to the new schema.
        let table: Database<PublicKeyCodec, NewUser> = env
            .open_database(&wtxn, Some(users::USERS_TABLE))
            .unwrap()
            .unwrap();
        let user = table.get(&wtxn, &pubkey).unwrap().unwrap();
        assert!(!user.disabled, "The user should not be disabled.");
        assert_eq!(user.used_bytes, 0, "The user should have 0 used bytes.");
    }
}
