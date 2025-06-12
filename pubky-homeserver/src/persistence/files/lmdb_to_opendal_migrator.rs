//!
//! TODO: Remove this whole module after the file migration is complete.
//!

use super::{FileIoError, FileService};
use crate::persistence::lmdb::{
    tables::files::{Entry, FileLocation},
    LmDB,
};
use crate::shared::webdav::EntryPath;
use futures_util::StreamExt;
use std::str::FromStr;

const BATCH_SIZE: usize = 100;

/// Migrate the files from the LMDB to the OpenDAL.
///
/// This is a temporary solution to migrate the files from the LMDB to the OpenDAL.
/// It will be removed after the migration is complete.
///
/// The migration is done in batches to avoid keeping a LMDB write transaction open for too long.
/// It will also update the entry location to use the OpenDAL.
#[derive(Debug, Clone)]
pub struct LmDbToOpendalMigrator {
    file_service: FileService,
    db: LmDB,
}

impl LmDbToOpendalMigrator {
    pub fn new(file_service: FileService, db: LmDB) -> Self {
        Self { file_service, db }
    }

    /// Migrate the files from the LMDB to the OpenDAL.
    ///
    /// This function will iterate over all the entries in the LMDB and migrate them to the OpenDAL.
    /// It will also update the entry location to use the OpenDAL.
    /// It tries to avoid keeping a lmd write transaction open for too long.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        let todo_count = self.count_lmdb_entries()?;
        if todo_count == 0 {
            tracing::debug!("[LMDB to OpenDAL] No entries to migrate");
            return Ok(());
        } else {
            tracing::info!(
                "[LMDB to OpenDAL] Starting migration. Found {} entries to migrate.",
                todo_count
            );
        }

        let mut count: usize = 0;
        while let Some(batch) = self.load_entry_batch()? {
            // Keep migrating until we have no more entries to migrate
            // Exact number can't be determined initially because new entries might be added
            // while we are migrating.
            // So we just keep migrating until we have no more entries to migrate.

            tracing::info!(
                "[LMDB to OpenDAL] Processing batch number {count} of {todo_count} entries",
                count = count,
                todo_count = todo_count
            );
            count += batch.len();
            for path in batch {
                if let Err(e) = self.process_single_entry(&path).await {
                    tracing::warn!("[LMDB to OpenDAL] Failed to migrate entry {}: {}. Continue with next entry.", path, e);
                }
            }
        }

        tracing::info!("[LMDB to OpenDAL] Migration completed successfully");
        Ok(())
    }

    /// Count the number of entries in the LMDB that need to be migrated.
    fn count_lmdb_entries(&self) -> anyhow::Result<usize> {
        let rtxn = self.db.env.read_txn()?;
        let mut counter: usize = 0;
        for key_value in self.db.tables.entries.iter(&rtxn)? {
            let (_, value) = key_value?;
            let entry = Entry::deserialize(value)?;
            if entry.file_location() == &FileLocation::LmDB {
                counter += 1;
            }
        }
        Ok(counter)
    }

    /// Load entries that need to be migrated. Max batch size is the maximum number of entries to load.
    fn load_entry_batch(&self) -> anyhow::Result<Option<Vec<EntryPath>>> {
        let rtxn = self.db.env.read_txn()?;
        let mut entries = Vec::new();
        for key_value in self.db.tables.entries.iter(&rtxn)? {
            let (key, value) = key_value?;
            let path = EntryPath::from_str(key)?;
            let entry = Entry::deserialize(value)?;
            if entry.file_location() != &FileLocation::LmDB {
                // Ignore entries that are already migrated
                continue;
            }
            entries.push(path);
            if entries.len() >= BATCH_SIZE {
                break;
            }
        }

        Ok(if entries.is_empty() {
            None
        } else {
            Some(entries)
        })
    }

    /// Migrate a single entry from LMDB to OpenDAL.
    async fn process_single_entry(&self, path: &EntryPath) -> anyhow::Result<()> {
        // Check if the entry changed since we last checked.
        // We are trying to use as little write txs as possible to avoid blocking the db
        // but this also implies that some entries might change in the meantime.
        let entry = match self.db.get_entry(path) {
            Ok(entry) => entry,
            Err(FileIoError::NotFound) => {
                tracing::debug!("[LMDB to OpenDAL] Skipping missing entry. File was deleted in the meantime: {}", path);
                return Ok(());
            }
            Err(e) => {
                tracing::error!("[LMDB to OpenDAL] Failed to get entry: {}: {}", path, e);
                return Err(e.into());
            }
        };

        if entry.file_location() != &FileLocation::LmDB {
            tracing::debug!(
                "[LMDB to OpenDAL] Skipping already migrated entry: {}",
                path
            );
            return Ok(());
        }

        // Step 1: Read file data from LMDB
        let stream = match self.file_service.get_stream(path).await {
            Ok(stream) => stream,
            Err(FileIoError::NotFound) => {
                tracing::debug!(
                    "[LMDB to OpenDAL] Skipping missing file. File was deleted in the meantime: {}",
                    path
                );
                return Ok(());
            }
            Err(e) => {
                return Err(e.into());
            }
        };
        // Write file data to OpenDAL
        let converted_stream = stream.map(|item| {
            item.map_err(|e| crate::persistence::files::WriteStreamError::Other(e.into()))
        });
        let metadata = self
            .file_service
            .opendal_service
            .write_stream(path, converted_stream, None)
            .await?;

        // Change the actual database. This needs to be done in a write tx to guarantee consistency.
        let mut wtx = self.db.env.write_txn()?;
        let mut locked_entry = match self.db.tables.entries.get(&wtx, path.as_str()) {
            Ok(Some(entry)) => Entry::deserialize(entry)?,
            _ => {
                tracing::warn!("[LMDB to OpenDAL] Entry not found or failed to parse in database: {}. Reverting migration.", path);
                wtx.abort(); // Abort transaction without committing
                self.file_service.opendal_service.delete(path).await?; // Delete the file from OpenDAL because migration failed.
                return Ok(());
            }
        };

        if locked_entry.file_location() != &FileLocation::LmDB {
            tracing::warn!(
                "[LMDB to OpenDAL] File was not in LMDB after migration: {}. Reverting.",
                path
            );
            wtx.abort(); // Abort transaction without committing
            self.file_service.opendal_service.delete(path).await?; // Delete the file from OpenDAL because migration failed.
            return Ok(());
        }

        if locked_entry.content_hash() != &metadata.hash {
            tracing::warn!("[LMDB to OpenDAL] Content hash mismatch after migration: {}. File must has changed in the meantime. Reverting.", path);
            wtx.abort(); // Abort transaction without committing
            self.file_service.opendal_service.delete(path).await?; // Delete the file from OpenDAL because migration failed.
            return Ok(());
        }

        // Update entry to point to OpenDAL.
        locked_entry.set_file_location(FileLocation::OpenDal);
        self.db
            .tables
            .entries
            .put(&mut wtx, path.as_ref(), &locked_entry.serialize())?;

        // Delete the file from LMDB.
        self.db.delete_file(&locked_entry.file_id(), &mut wtx)?;
        wtx.commit()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        persistence::files::FileIoError,
        shared::webdav::{EntryPath, WebDavPath},
        storage_config::StorageConfigToml,
        ConfigToml,
    };
    use bytes::Bytes;
    use futures_util::StreamExt;
    use std::path::Path;

    #[tokio::test]
    async fn test_lmdb_migrate_file() {
        let config = ConfigToml::test();
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone()).unwrap();

        // Create a test user
        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        // Create a test file path
        let path = EntryPath::new(pubkey, WebDavPath::new("/pub/test_file.txt").unwrap());

        // Test data to write
        let test_data = b"Hello, LMDB to OpenDAL migration! This is a test file content.";
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);

        // Write the file to LMDB initially
        let entry = file_service
            .write_stream(&path, FileLocation::LmDB, stream)
            .await
            .unwrap();

        // Verify the file was written to LMDB
        assert_eq!(*entry.file_location(), FileLocation::LmDB);
        assert_eq!(entry.content_length(), test_data.len());

        // Verify we can read the file from LMDB
        let content_before = file_service.get(&path).await.unwrap();
        assert_eq!(content_before.as_ref(), test_data);

        // Create the migrator and run migration
        let migrator = LmDbToOpendalMigrator::new(file_service.clone(), db.clone());
        migrator.migrate().await.unwrap();

        // Verify the entry was updated in the database
        let migrated_entry = db.get_entry(&path).unwrap();
        assert_eq!(
            *migrated_entry.file_location(),
            FileLocation::OpenDal,
            "File location should be updated to OpenDAL"
        );
        assert_eq!(
            migrated_entry.content_length(),
            test_data.len(),
            "Content length should remain the same"
        );
        assert_eq!(
            migrated_entry.content_hash(),
            entry.content_hash(),
            "Content hash should remain the same"
        );

        // Verify we can still read the file (now from OpenDAL)
        let content_after = file_service.get(&path).await.unwrap();
        assert_eq!(
            content_after.as_ref(),
            test_data,
            "Content should be identical after migration"
        );

        // Verify the file exists in OpenDAL
        assert!(
            file_service.opendal_service.exists(&path).await.unwrap(),
            "File should exist in OpenDAL after migration"
        );

        // Verify we can read the file stream after migration
        let mut stream = file_service.get_stream(&path).await.unwrap();
        let mut collected_data = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected_data.extend_from_slice(&chunk);
        }
        assert_eq!(
            collected_data,
            test_data.to_vec(),
            "Streamed content should match original after migration"
        );

        // Verify that the blob was deleted from LMDB
        let id = migrated_entry.file_id();
        match db.read_file(&id).await {
            Ok(_) => {
                panic!("File should be deleted from LMDB after migration");
            }
            Err(e) => {
                assert_eq!(e.to_string(), FileIoError::NotFound.to_string());
            }
        }

        // Verify that running migration again doesn't cause issues (idempotency test)
        migrator.migrate().await.unwrap();
        let final_entry = db.get_entry(&path).unwrap();
        assert_eq!(
            *final_entry.file_location(),
            FileLocation::OpenDal,
            "File should still be in OpenDAL after second migration"
        );
    }

    #[tokio::test]
    async fn test_lmdb_migrate_no_files() {
        // Set up test environment with in-memory storage
        let mut config = ConfigToml::test();
        config.storage = StorageConfigToml::InMemory;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone()).unwrap();

        // Create the migrator and run migration on empty database
        let migrator = LmDbToOpendalMigrator::new(file_service, db.clone());

        // This should complete without error even when there are no files to migrate
        migrator.migrate().await.unwrap();

        // Verify the count method returns 0
        assert_eq!(migrator.count_lmdb_entries().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_lmdb_migrate_mixed_files() {
        // Set up test environment with in-memory storage
        let mut config = ConfigToml::test();
        config.storage = StorageConfigToml::InMemory;
        let db = LmDB::test();
        let file_service =
            FileService::new_from_config(&config, Path::new("/tmp/test"), db.clone()).unwrap();

        // Create a test user
        let pubkey = pkarr::Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();

        // Create test data
        let test_data = b"Test data for mixed migration";

        // Create a file in LMDB
        let lmdb_path = EntryPath::new(
            pubkey.clone(),
            WebDavPath::new("/pub/lmdb_file.txt").unwrap(),
        );
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);
        let lmdb_entry = file_service
            .write_stream(&lmdb_path, FileLocation::LmDB, stream)
            .await
            .unwrap();

        // Create a file already in OpenDAL
        let opendal_path =
            EntryPath::new(pubkey, WebDavPath::new("/pub/opendal_file.txt").unwrap());
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);
        let opendal_entry = file_service
            .write_stream(&opendal_path, FileLocation::OpenDal, stream)
            .await
            .unwrap();

        // Verify initial states
        assert_eq!(*lmdb_entry.file_location(), FileLocation::LmDB);
        assert_eq!(*opendal_entry.file_location(), FileLocation::OpenDal);

        // Count LMDB entries before migration
        let migrator = LmDbToOpendalMigrator::new(file_service.clone(), db.clone());
        assert_eq!(
            migrator.count_lmdb_entries().unwrap(),
            1,
            "Should count only the LMDB file"
        );

        // Run migration
        migrator.migrate().await.unwrap();

        // Verify the LMDB file was migrated
        let migrated_lmdb_entry = db.get_entry(&lmdb_path).unwrap();
        assert_eq!(
            *migrated_lmdb_entry.file_location(),
            FileLocation::OpenDal,
            "LMDB file should be migrated to OpenDAL"
        );

        // Verify the OpenDAL file was left unchanged
        let unchanged_opendal_entry = db.get_entry(&opendal_path).unwrap();
        assert_eq!(
            *unchanged_opendal_entry.file_location(),
            FileLocation::OpenDal,
            "OpenDAL file should remain in OpenDAL"
        );

        // Verify no more LMDB entries to migrate
        assert_eq!(
            migrator.count_lmdb_entries().unwrap(),
            0,
            "Should be no more LMDB files to migrate"
        );
    }
}
