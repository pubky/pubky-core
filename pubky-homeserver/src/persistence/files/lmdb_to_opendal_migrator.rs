use crate::persistence::lmdb::{LmDB, tables::files::{Entry, FileLocation}};
use crate::shared::webdav::EntryPath;
use super::FileService;
use std::{str::FromStr, time::Duration};
use futures_util::StreamExt;



const BATCH_SIZE: usize = 20;


/// The file service creates an abstraction layer over the LMDB and OpenDAL services.
/// This way, files can be managed in a unified way.
#[derive(Debug, Clone)]
pub struct LmDbToOpendalMigrator {
    file_service: FileService,
    db: LmDB,
}

impl LmDbToOpendalMigrator {
    pub fn new(file_service: FileService, db: LmDB) -> Self {
        Self {
            file_service,
            db,
        }
    }

    /// Migrate the files from the LMDB to the OpenDAL.
    ///
    /// This function will iterate over all the entries in the LMDB and migrate them to the OpenDAL.
    /// It will also update the entry location to use the OpenDAL.
    /// It tries to avoid keeping a lmd write transaction open for too long.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        tracing::info!("Starting LMDB to OpenDAL migration");
        let todo_count = self.count_lmdb_entries()?;
        if todo_count == 0 {
            tracing::info!("No entries to migrate");
            return Ok(());
        } else {
            let predicted_batch_count = (todo_count as f64 / BATCH_SIZE as f64).ceil() as usize;
            tracing::info!("Found {} entries to migrate. Predicted batch count: {}", todo_count, predicted_batch_count);
        }

        let mut count: usize = 0;
        while let Some(batch) = self.load_entry_batch()? {
            // Keep migrating until we have no more entries to migrate
            // Exact number can't be determined initially because new entries might be added
            // while we are migrating.
            // So we just keep migrating until we have no more entries to migrate.

            tracing::info!("Processing batch number {count} of {todo_count} entries", count = count, todo_count = todo_count);
            count += batch.len();
            for path in batch {
                if let Err(e) = self.process_single_entry(&path).await {
                    tracing::error!("Failed to migrate entry {}: {}. Continue with next entry.", path, e);
                }
            }
            // Sleep to give the db a chance to do other things.
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        tracing::info!("Migration completed successfully");
        Ok(())
    }

    /// Count the number of entries in the LMDB that need to be migrated.
    fn count_lmdb_entries(&self) -> anyhow::Result<usize> {
        let rtxn = self.db.env.read_txn()?;
        let mut counter: usize = 0;
        for key_value in self.db.tables.entries.iter(&rtxn)? {
            let (_, value) = key_value?;
            let entry = Entry::deserialize(&value)?;
            if entry.file_location() == &FileLocation::LMDB {
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
            let entry = Entry::deserialize(&value)?;
            if entry.file_location() != &FileLocation::LMDB {
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
        let entry = if let Some(entry) = self.db.get_entry(&path)? {
            entry
        } else {
            tracing::debug!("Skipping missing entry: {}", path);
            return Ok(());
        };

        if entry.file_location() != &FileLocation::LMDB {
            tracing::debug!("Skipping already migrated entry: {}", path);
            return Ok(());
        }

        // Step 1: Read file data from LMDB
        let stream = self.file_service.get_stream(&path).await
            .map_err(|e| anyhow::anyhow!("Failed to get file stream from LMDB for {}: {}", path, e))?;

        // Write file data to OpenDAL
        let converted_stream = stream.map(|item| item.map_err(|e| crate::persistence::files::write_stream_error::WriteStreamError::Other(e.into())));
        let metadata = self.file_service.opendal_service.write_stream(&path, converted_stream).await?;

        // Change the actual database. This needs to be done in a write tx to guarantee consistency.
        let mut wtx = self.db.env.write_txn()?;
        let mut locked_entry = match self.db.tables.entries.get(&wtx, path.as_str()) {
            Ok(Some(entry)) => {
                Entry::deserialize(&entry)?
            },
            _ => {
                tracing::error!("Entry not found or failed to parse in database: {}. Reverting migration.", path);
                wtx.commit()?; // Close write tx as we are not going to use it.
                self.file_service.opendal_service.delete(&path).await?; // Delete the file from OpenDAL because migration failed.
                return Ok(());
            }
        };

        if locked_entry.file_location() != &FileLocation::LMDB {
            tracing::error!("File was not in LMDB after migration: {}. Reverting.", path);
            wtx.commit()?; // Close write tx as we are not going to use it.
            self.file_service.opendal_service.delete(&path).await?; // Delete the file from OpenDAL because migration failed.
            return Ok(());
        }

        if locked_entry.content_hash() != &metadata.hash {
            tracing::error!("Content hash mismatch after migration: {}. File must has changed in the meantime. Reverting.", path);
            wtx.commit()?; // Close write tx as we are not going to use it.
            self.file_service.opendal_service.delete(&path).await?; // Delete the file from OpenDAL because migration failed.
            return Ok(());
        }

        locked_entry.set_file_location(FileLocation::OpenDal);

        self.db.tables.entries.put(&mut wtx, &path.to_string(), &locked_entry.serialize())?;
        wtx.commit()?;
        
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lmdb_migrate_file() {

    }
}