use crate::{
    persistence::{
        files::{FileIoError, FileMetadata},
        sql::{
            entry::{EntryEntity, EntryRepository},
            event::{EventEntity, EventRepository, EventType},
            user::UserRepository,
            SqlDb, UnifiedExecutor,
        },
    },
    shared::webdav::EntryPath,
};
use tokio::sync::broadcast::{self, error::SendError};

#[derive(Debug, Clone)]
pub struct EntryService {
    db: SqlDb,
    // user_disk_space_quota_bytes: u64,
    event_tx: broadcast::Sender<EventEntity>,
}

impl EntryService {
    pub fn new(db: SqlDb, event_tx: broadcast::Sender<EventEntity>) -> Self {
        Self { db, event_tx }
    }

    pub fn db(&self) -> &SqlDb {
        &self.db
    }

    /// Write an entry to the database.
    ///
    /// This includes all associated operations:
    /// - Write a public [Event]
    /// - Write the entry to the database
    pub async fn write_entry<'a>(
        &self,
        path: &EntryPath,
        metadata: &FileMetadata,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EntryEntity, FileIoError> {
        let existing_entry = match EntryRepository::get_by_path(path, executor).await {
            Ok(entry) => Some(entry),
            Err(sqlx::Error::RowNotFound) => None,
            Err(e) => return Err(e.into()),
        };

        // Create/Update entry
        let entry = if let Some(existing_entry) = existing_entry {
            self.update_entry(existing_entry, metadata, executor)
                .await?
        } else {
            self.create_entry(path, metadata, executor).await?
        };

        // Create event and broadcast
        let event = EventRepository::create(
            entry.user_id,
            EventType::Put,
            path,
            Some(metadata.hash),
            executor,
        )
        .await?;
        // Broadcast to any listening clients.
        match self.event_tx.send(event) {
            Ok(_) => {}
            Err(SendError(_)) => {
                // No active receivers - this is expected when no clients are long-polling
            }
        }
        Ok(entry)
    }

    /// Update an existing entry in the database.
    async fn update_entry<'a>(
        &self,
        mut entry: EntryEntity,
        metadata: &FileMetadata,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EntryEntity, FileIoError> {
        entry.content_hash = metadata.hash;
        entry.content_length = metadata.length as u64;
        entry.content_type = metadata.content_type.clone();

        EntryRepository::update(&entry, executor).await?;

        Ok(entry)
    }

    /// Create a new entry in the database.
    async fn create_entry<'a>(
        &self,
        path: &EntryPath,
        metadata: &FileMetadata,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EntryEntity, FileIoError> {
        let user_id = UserRepository::get_id(path.pubkey(), executor).await?;
        let entry_id = EntryRepository::create(
            user_id,
            path.path(),
            &metadata.hash,
            metadata.length as u64,
            &metadata.content_type,
            executor,
        )
        .await?;
        let entry = EntryRepository::get(entry_id, executor).await?;
        Ok(entry)
    }

    /// Delete an entry from the database.
    ///
    /// This includes all associated operations:
    /// - Write a public [Event]
    /// - Delete the entry from the database
    ///
    pub async fn delete_entry<'a>(
        &self,
        path: &EntryPath,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), FileIoError> {
        let entry = match EntryRepository::get_by_path(path, executor).await {
            Ok(entry) => entry,
            Err(sqlx::Error::RowNotFound) => return Err(FileIoError::NotFound),
            Err(e) => return Err(e.into()),
        };

        EntryRepository::delete(entry.id, executor).await?;

        // Create event and broadcast
        let event =
            EventRepository::create(entry.user_id, EventType::Delete, path, None, executor).await?;
        // Broadcast to any listening clients.
        match self.event_tx.send(event) {
            Ok(_) => {} // Successfully broadcast to receivers
            Err(SendError(_)) => {
                // No active receivers - this is expected when no clients are long-polling
            }
        }

        Ok(())
    }
}
