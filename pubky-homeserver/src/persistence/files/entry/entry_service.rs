use crate::{
    persistence::{
        files::{FileIoError, FileMetadata},
        sql::{
            entry::{EntryEntity, EntryRepository},
            UnifiedExecutor,
        },
    },
    shared::webdav::EntryPath,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct EntryService;

impl EntryService {
    pub fn new() -> Self {
        Self
    }

    /// Write an entry to the database.
    ///
    /// Returns the entry.
    pub async fn write_entry<'a>(
        &self,
        user_id: i32,
        existing_entry: Option<EntryEntity>,
        path: &EntryPath,
        metadata: &FileMetadata,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EntryEntity, FileIoError> {
        // Create/Update entry
        let entry = if let Some(existing_entry) = existing_entry {
            self.update_entry(existing_entry, metadata, executor)
                .await?
        } else {
            self.create_entry(user_id, path, metadata, executor).await?
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
        user_id: i32,
        path: &EntryPath,
        metadata: &FileMetadata,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EntryEntity, FileIoError> {
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

    /// Delete an entry and return its metadata for transactional accounting.
    pub async fn delete_entry<'a>(
        &self,
        path: &EntryPath,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EntryEntity, FileIoError> {
        let entry = match EntryRepository::get_by_path(path, executor).await {
            Ok(entry) => entry,
            Err(sqlx::Error::RowNotFound) => return Err(FileIoError::NotFound),
            Err(e) => return Err(e.into()),
        };

        EntryRepository::delete(entry.id, executor).await?;

        Ok(entry)
    }
}
