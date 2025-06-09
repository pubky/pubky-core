use pkarr::PublicKey;
use pubky_common::timestamp::Timestamp;

use crate::{
    persistence::{
        files::{FileIoError, FileMetadata, WriteStreamError},
        lmdb::{
            tables::{
                events::Event,
                files::{Entry, FileLocation},
            },
            LmDB,
        },
    },
    shared::webdav::EntryPath,
    AppContext,
};

pub struct EntryService {
    db: LmDB,
    user_disk_space_quota_bytes: Option<u64>,
}

impl EntryService {
    pub fn new(context: &AppContext) -> Self {
        let bytes_quota = context.config_toml.general.user_storage_quota_mb * 1024 * 1024;
        let bytes_quota = if bytes_quota == 0 {
            None
        } else {
            Some(bytes_quota)
        };
        Self {
            db: context.db.clone(),
            user_disk_space_quota_bytes: bytes_quota,
        }
    }

    /// Write an entry to the database.
    ///
    /// This includes all associated operations:
    /// - Update user data usage
    /// - Write a public [Event]
    /// - Write the entry to the database
    ///
    /// If the user exceeds the disk space quota, this will return a [WriteStreamError::DiskSpaceQuotaExceeded] error.
    pub fn write_entry(
        &self,
        path: &EntryPath,
        metadata: &FileMetadata,
    ) -> Result<Entry, FileIoError> {
        let mut wtxn = self.db.env.write_txn()?;

        // Get old entry size. If it doesn't exist, use 0.
        let old_entry_size = self
            .db
            .tables
            .entries
            .get(&wtxn, path.as_str())?
            .map(|bytes| Entry::deserialize(bytes).map(|entry| entry.content_length()))
            .transpose()?
            .unwrap_or(0);

        // Write entry
        let mut entry = Entry::new();
        entry.set_content_hash(metadata.hash);
        entry.set_content_length(metadata.length);
        entry.set_timestamp(&metadata.modified_at);
        entry.set_file_location(FileLocation::OpenDal);
        let entry_key = path.to_string();
        self.db
            .tables
            .entries
            .put(&mut wtxn, entry_key.as_str(), &entry.serialize())?;

        // Update user data usage
        let mut user = self
            .db
            .tables
            .users
            .get(&wtxn, path.pubkey())?
            .ok_or(FileIoError::NotFound)?;
        user.used_bytes = user
            .used_bytes
            .saturating_add(metadata.length as u64)
            .saturating_sub(old_entry_size as u64);

        if let Some(quota) = self.user_disk_space_quota_bytes {
            if user.used_bytes > quota {
                return Err(FileIoError::StreamBroken(
                    WriteStreamError::DiskSpaceQuotaExceeded,
                ));
            }
        }

        self.db.tables.users.put(&mut wtxn, path.pubkey(), &user)?;

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
    /// - Update user data usage
    /// - Write a public [Event]
    /// - Delete the entry from the database
    /// 
    pub fn delete_entry(&self, path: &EntryPath) -> Result<(), FileIoError> {
        let mut wtxn = self.db.env.write_txn()?;

        // Update the data usage counter of the user
        let entry_bytes = self
            .db
            .tables
            .entries
            .get(&wtxn, path.as_str())?
            .ok_or(FileIoError::NotFound)?;
        let entry = Entry::deserialize(entry_bytes)?;
        let mut user = self
            .db
            .tables
            .users
            .get(&wtxn, path.pubkey())?
            .ok_or(FileIoError::NotFound)?;
        user.used_bytes = user
            .used_bytes
            .saturating_sub(entry.content_length() as u64);
        self.db.tables.users.put(&mut wtxn, path.pubkey(), &user)?;

        // Delete entry
        self.db.tables.entries.delete(&mut wtxn, path.as_str())?;

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
