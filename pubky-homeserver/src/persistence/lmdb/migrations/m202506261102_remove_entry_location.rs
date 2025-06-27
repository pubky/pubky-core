///
/// This migration removes the `file_location` field from the `entries` table.
/// The `file_location` field was used to store the location of the file in the file system.
/// This was used to help the migration of the files from the LMDB to opendal.
/// It's not needed
///
use crate::persistence::lmdb::tables::entries::{EntryHash, ENTRIES_TABLE};
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, RwTxn};
use postcard::{from_bytes, to_allocvec};
use pubky_common::timestamp::Timestamp;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// The location of the file.
/// This is used to determine where the file is stored.
/// Used during the transition process from LMDB to OpenDAL.
/// TODO: Remove after the file migration is complete.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq, Default)]
pub enum FileLocation {
    #[default]
    LmDB,
    OpenDal,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct OldEntry {
    version: usize,
    timestamp: Timestamp,
    content_hash: EntryHash,
    content_length: usize,
    content_type: String,
    file_location: FileLocation,
}

impl BytesEncode<'_> for OldEntry {
    type EItem = Self;

    fn bytes_encode(user: &Self::EItem) -> Result<Cow<[u8]>, BoxedError> {
        let vec = to_allocvec(user).unwrap();
        Ok(Cow::Owned(vec))
    }
}

impl<'a> BytesDecode<'a> for OldEntry {
    type DItem = Self;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let user: OldEntry = from_bytes(bytes).unwrap();
        Ok(user)
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct NewEntry {
    /// Encoding version
    version: usize,
    /// Modified at
    timestamp: Timestamp,
    content_hash: EntryHash,
    content_length: usize,
    content_type: String,
}

impl BytesEncode<'_> for NewEntry {
    type EItem = Self;

    fn bytes_encode(new_entry: &Self::EItem) -> Result<Cow<[u8]>, BoxedError> {
        let vec = to_allocvec(new_entry)?;

        Ok(Cow::Owned(vec))
    }
}

impl<'a> BytesDecode<'a> for NewEntry {
    type DItem = Self;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let user: NewEntry = from_bytes(bytes)?;

        Ok(user)
    }
}

impl From<OldEntry> for NewEntry {
    fn from(old_entry: OldEntry) -> Self {
        Self {
            version: old_entry.version,
            timestamp: old_entry.timestamp,
            content_hash: old_entry.content_hash,
            content_length: old_entry.content_length,
            content_type: old_entry.content_type,
        }
    }
}

/// Checks if the migration is needed.
/// Tries to read entries with the new schema. If it succeeds, the migration is not needed.
/// If it fails, the migration is needed.
fn is_migration_needed(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<bool> {
    let new_table: Database<heed::types::Str, NewEntry> = env
        .open_database(wtxn, Some(ENTRIES_TABLE))?
        .expect("Entries database is not available");

    match new_table.first(wtxn) {
        Ok(Some(_entry)) => {
            // Entry found. The new schema is valid.
            // Migrations has already been run.
            Ok(false)
        }
        Ok(None) => {
            // No entries found. No need to run the migration.
            Ok(false)
        }
        Err(_e) => {
            // Failed to deserialize. It's the old schema.
            // Migrations is needed.
            Ok(true)
        }
    }
}

fn read_old_entries_table(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<Vec<(String, OldEntry)>> {
    let table: Database<heed::types::Str, OldEntry> = env
        .open_database(wtxn, Some(ENTRIES_TABLE))?
        .expect("Entries database is not available");

    let mut new_entries: Vec<(String, OldEntry)> = vec![];
    for entry in table.iter(wtxn)? {
        let (key, old_user) = entry?;
        new_entries.push((key.to_string(), old_user));
    }

    Ok(new_entries)
}

fn write_new_entries_table(
    env: &Env,
    wtxn: &mut RwTxn,
    entries: Vec<(String, NewEntry)>,
) -> anyhow::Result<()> {
    let table: Database<heed::types::Str, NewEntry> = env
        .open_database(wtxn, Some(ENTRIES_TABLE))?
        .expect("Entries database is not available");

    for (key, new_entry) in entries {
        table.put(wtxn, &key, &new_entry)?;
    }

    Ok(())
}

pub fn run(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    if !is_migration_needed(env, wtxn)? {
        return Ok(());
    }

    tracing::info!("Running migration m202506261102_remove_entry_location");
    let old_entries = read_old_entries_table(env, wtxn)
        .map_err(|e| anyhow::anyhow!("Failed to read old users table: {}", e))?;

    // Migrate the users to the new schema.
    let new_entries: Vec<(String, NewEntry)> = old_entries
        .into_iter()
        .map(|(key, old_entry)| (key, old_entry.into()))
        .collect();

    tracing::info!("Read {} entries", new_entries.len());
    write_new_entries_table(env, wtxn, new_entries)
        .map_err(|e| anyhow::anyhow!("Failed to write new entries table: {}", e))?;

    tracing::info!("Successfully migrated");

    Ok(())
}

#[cfg(test)]
mod tests {
    use heed::EnvOpenOptions;
    use crate::persistence::lmdb::{db::DEFAULT_MAP_SIZE, migrations::m0};
    use super::*;


    #[test]
    fn test_is_migration_needed_no() {
        let old = OldEntry {
            version: 1,
            timestamp: Timestamp::now(),
            content_hash: EntryHash::default(),
            content_length: 0,
            content_type: "text/plain".to_string(),
            file_location: FileLocation::LmDB,
        };
        old.
    }

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

        // Write an entry to the old table.
        let table: Database<heed::types::Str, OldEntry> =
            env.create_database(&mut wtxn, Some(ENTRIES_TABLE)).unwrap();
        table
            .put(
                &mut wtxn,
                "pubky/test.txt",
                &OldEntry {
                    version: 1,
                    timestamp: Timestamp::now(),
                    content_hash: EntryHash::default(),
                    content_length: 0,
                    content_type: "text/plain".to_string(),
                    file_location: FileLocation::LmDB,
                },
            )
            .unwrap();
        wtxn.commit().unwrap();
        let mut wtxn = env.write_txn().unwrap();
        assert!(is_migration_needed(&env, &mut wtxn).unwrap());
    }

    #[test]
    fn test_is_migration_needed_no_entries() {
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
        let _: Database<heed::types::Str, OldEntry> =
            env.create_database(&mut wtxn, Some(ENTRIES_TABLE)).unwrap();

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
        let table: Database<heed::types::Str, NewEntry> =
            env.create_database(&mut wtxn, Some(ENTRIES_TABLE)).unwrap();
        table
            .put(
                &mut wtxn,
                "pubky/test.txt",
                &NewEntry {
                    version: 1,
                    timestamp: Timestamp::now(),
                    content_hash: EntryHash::default(),
                    content_length: 0,
                    content_type: "text/plain".to_string(),
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

        // Write a entry to the old table.
        let table: Database<heed::types::Str, OldEntry> =
            env.create_database(&mut wtxn, Some(ENTRIES_TABLE)).unwrap();
        let old_entry = OldEntry {
            version: 1,
            timestamp: Timestamp::now(),
            content_hash: EntryHash::default(),
            content_length: 0,
            content_type: "text/plain".to_string(),
            file_location: FileLocation::LmDB,
        };
        table.put(&mut wtxn, "pubky/test.txt", &old_entry).unwrap();

        // Migrate the users to the new schema.
        run(&env, &mut wtxn).unwrap();

        // Check that the user has been migrated to the new schema.
        let table: Database<heed::types::Str, NewEntry> = env
            .open_database(&wtxn, Some(ENTRIES_TABLE))
            .unwrap()
            .unwrap();
        let entry = table.get(&wtxn, "pubky/test.txt").unwrap().unwrap();
        assert_eq!(
            entry.version, old_entry.version,
            "The version should be the same."
        );
        assert_eq!(
            entry.timestamp, old_entry.timestamp,
            "The timestamp should be the same."
        );
        assert_eq!(
            entry.content_hash, old_entry.content_hash,
            "The content hash should be the same."
        );
        assert_eq!(
            entry.content_length, old_entry.content_length,
            "The content length should be the same."
        );
        assert_eq!(
            entry.content_type, old_entry.content_type,
            "The content type should be the same."
        );
    }
}
