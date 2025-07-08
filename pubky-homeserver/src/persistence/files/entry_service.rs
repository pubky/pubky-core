use pubky_common::timestamp::Timestamp;

use crate::{
    persistence::{
        files::{FileIoError, FileMetadata},
        lmdb::{
            tables::{entries::Entry, events::Event},
            LmDB,
        },
    },
    shared::webdav::EntryPath,
};

#[derive(Debug, Clone)]
pub struct EntryService {
    db: LmDB,
    // user_disk_space_quota_bytes: u64,
}

impl EntryService {
    pub fn new(db: LmDB) -> Self {
        Self { db }
    }

    /// Write an entry to the database.
    ///
    /// This includes all associated operations:
    /// - Write a public [Event]
    /// - Write the entry to the database
    pub fn write_entry(
        &self,
        path: &EntryPath,
        metadata: &FileMetadata,
    ) -> Result<Entry, FileIoError> {
        let mut wtxn = self.db.env.write_txn()?;

        // Write entry
        let mut entry = Entry::new();
        entry.set_content_hash(metadata.hash);
        entry.set_content_length(metadata.length);
        entry.set_timestamp(&metadata.modified_at);
        entry.set_content_type(metadata.content_type.clone());
        let entry_key = path.to_string();
        self.db
            .tables
            .entries
            .put(&mut wtxn, entry_key.as_str(), &entry.serialize())?;

        // Write a public [Event].
        let url = format!("pubky://{}", entry_key);
        let event = Event::put(&url);
        let value = event.serialize();
        self.db
            .tables
            .events
            .put(&mut wtxn, metadata.modified_at.to_string().as_str(), &value)?;

        wtxn.commit()?;

        Ok(entry)
    }

    /// Delete an entry from the database.
    ///
    /// This includes all associated operations:
    /// - Write a public [Event]
    /// - Delete the entry from the database
    ///
    pub fn delete_entry(&self, path: &EntryPath) -> Result<(), FileIoError> {
        let mut wtxn = self.db.env.write_txn()?;

        // Delete entry
        let deleted = self.db.tables.entries.delete(&mut wtxn, path.as_str())?;
        if !deleted {
            return Err(FileIoError::NotFound);
        }

        // create DELETE event
        let url = format!("pubky://{}", path.as_str());
        let event = Event::delete(&url);
        let value = event.serialize();
        let key = Timestamp::now().to_string();
        self.db.tables.events.put(&mut wtxn, &key, &value)?;

        wtxn.commit()?;
        Ok(())
    }
}
