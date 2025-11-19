use crate::{
    persistence::{
        files::{FileIoError, FileMetadata},
        sql::{
            entry::{EntryEntity, EntryRepository},
            user::UserRepository,
            SqlDb, UnifiedExecutor,
        },
    },
    shared::webdav::EntryPath,
};

#[derive(Debug, Clone)]
pub struct EntryService {
    db: SqlDb,
}

impl EntryService {
    pub fn new(db: SqlDb) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &SqlDb {
        &self.db
    }

    /// Write an entry to the database.
    ///
    /// Returns the entry.
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

        Ok(())
    }
}
