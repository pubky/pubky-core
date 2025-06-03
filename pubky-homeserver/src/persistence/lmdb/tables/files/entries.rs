use super::super::events::Event;
use super::{super::super::LmDB, InDbFileId, InDbTempFile};
use crate::constants::{DEFAULT_LIST_LIMIT, DEFAULT_MAX_LIST_LIMIT};
use crate::persistence::files::{FileIoError, FileMetadata};
use crate::shared::webdav::EntryPath;
use heed::{
    types::{Bytes, Str},
    Database, RoTxn,
};
use postcard::{from_bytes, to_allocvec};
use pubky_common::{crypto::Hash, timestamp::Timestamp};
use serde::{Deserialize, Serialize};
use tokio::task::JoinError;
use tracing::instrument;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;
pub const ENTRIES_TABLE: &str = "entries";


impl LmDB {

    /// Check if an entry exists.
    pub fn entry_exists(&self, path: &EntryPath) -> Result<bool, FileIoError> {
        match self.get_entry(path) {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.to_string() == FileIoError::NotFound.to_string() {
                    Ok(false)
                } else {
                    Err(e)
                }
            }   
        }
    }

    /// Writes an entry to the database.
    ///
    /// The entry is written to the database and the file is written to the blob store.
    /// An event is written to the events table.
    /// The entry is returned.
    pub async fn write_entry_from_file(
        &mut self,
        path: &EntryPath,
        file: &InDbTempFile,
    ) -> Result<Entry, FileIoError> {
        let mut db = self.clone();
        let path = path.clone();
        let file = file.clone();
        let join_handle = tokio::task::spawn_blocking(move || -> Result<Entry, FileIoError> {
            db.write_entry_from_file_sync(&path, &file)
        })
        .await;
        match join_handle {
            Ok(result) => result,
            Err(e) => {
                // Happens when the task panics or gets cancelled.
                // Should only happen if the homeserver is shutdown.
                // Returning not found is a safe fallback.
                tracing::error!("Error writing entry. JoinError: {:?}", e);
                Err(FileIoError::NotFound)
            }
        }
    }

    /// Write an entry without writing the file to the blob store.
    pub fn write_entry(
        &self,
        path: &EntryPath,
        metadata: &FileMetadata,
        file_location: FileLocation,
    ) -> Result<Entry, FileIoError> {
        let mut wtxn = self.env.write_txn()?;
        let mut entry = Entry::new();
        entry.set_content_hash(metadata.hash.clone());
        entry.set_content_length(metadata.length);
        entry.set_timestamp(&metadata.modified_at);
        entry.file_location = file_location;
        let entry_key = path.to_string();
        self.tables
            .entries
            .put(&mut wtxn, entry_key.as_str(), &entry.serialize())?;

        // TODO: Extract this to a separate function.
        // Write a public [Event].
        let url = format!("pubky://{}", entry_key);
        let event = Event::put(&url);
        let value = event.serialize();

        self.tables
            .events
            .put(&mut wtxn, metadata.modified_at.to_string().as_str(), &value)?;
        wtxn.commit()?;

        Ok(entry)
    }

    /// Writes an entry to the database.
    ///
    /// The entry is written to the database and the file is written to the blob store.
    /// An event is written to the events table.
    /// The entry is returned.
    pub fn write_entry_from_file_sync(
        &mut self,
        path: &EntryPath,
        file: &InDbTempFile,
    ) -> Result<Entry, FileIoError> {
        let mut wtxn = self.env.write_txn()?;
        let mut metadata = file.metadata().clone();
        let file_id = self.write_file_sync(file, &mut wtxn)?;
        wtxn.commit()?;
        metadata.modified_at = file_id.timestamp().clone();
        let entry = self.write_entry(path, &metadata, FileLocation::LMDB)?;
        Ok(entry)
    }

    /// Get an entry from the database.
    /// This doesn't include the file but only metadata.
    pub fn get_entry(&self, path: &EntryPath) -> Result<Entry, FileIoError> {
        let txn = self.env.read_txn()?;
        let entry = match self.tables.entries.get(&txn, path.as_str())? {
            Some(bytes) => Entry::deserialize(bytes)?,
            None => return Err(FileIoError::NotFound),
        };
        Ok(entry)
    }

    /// Delete an entry including the associated file from the database.
    pub async fn delete_entry(&self, path: &EntryPath) -> Result<(), FileIoError> {
        let mut wtxn = self.env.write_txn()?;

        let deleted = self.tables.entries.delete(&mut wtxn, path.as_str())?;
        if !deleted {
            wtxn.abort();
            return Err(FileIoError::NotFound);
        }

        // create DELETE event
        let url = format!("pubky://{}", path.as_str());
        let event = Event::delete(&url);
        let value = event.serialize();
        let key = Timestamp::now().to_string();
        self.tables.events.put(&mut wtxn, &key, &value)?;
        wtxn.commit()?;
        Ok(())
    }

    /// Delete an entry including the associated file from the database.
    pub async fn delete_entry_and_file(&self, path: &EntryPath) -> Result<(), FileIoError> {
        let db = self.clone();
        let path = path.clone();
        let join_handle = tokio::task::spawn_blocking(move || -> Result<(), FileIoError> {
            db.delete_entry_and_file_sync(&path)
        })
        .await;
        match join_handle {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Error deleting entry. JoinError: {:?}", e);
                Err(FileIoError::NotFound)
            }
        }
    }

    /// Delete an entry including the associated file from the database.
    pub fn delete_entry_and_file_sync(&self, path: &EntryPath) -> Result<(), FileIoError> {
        let entry = self.get_entry(path)?;

        let mut wtxn = self.env.write_txn()?;
        self.delete_file(&entry.file_id(), &mut wtxn)?;

        let deleted = self.tables.entries.delete(&mut wtxn, path.as_str())?;
        if !deleted {
            wtxn.abort();
            return Err(FileIoError::NotFound);
        }

        // create DELETE event
        let url = format!("pubky://{}", path.as_str());

        let event = Event::delete(&url);
        let value = event.serialize();

        let key = Timestamp::now().to_string();

        self.tables.events.put(&mut wtxn, &key, &value)?;

        wtxn.commit()?;
        Ok(())
    }

    /// Bytes stored at `path` for this user (0 if none).
    pub fn get_entry_content_length(&self, path: &EntryPath) -> Result<u64, FileIoError> {
        let content_length = self
            .get_entry(path)?
            .content_length() as u64;
        Ok(content_length)
    }

    pub fn contains_directory(&self, txn: &RoTxn, entry_path: &EntryPath) -> anyhow::Result<bool> {
        Ok(self
            .tables
            .entries
            .get_greater_than(txn, entry_path.as_str())?
            .is_some())
    }

    /// Return a list of pubky urls.
    ///
    /// - limit defaults to [crate::config::DEFAULT_LIST_LIMIT] and capped by [crate::config::DEFAULT_MAX_LIST_LIMIT]
    pub fn list_entries(
        &self,
        txn: &RoTxn,
        entry_path: &EntryPath,
        reverse: bool,
        limit: Option<u16>,
        cursor: Option<String>,
        shallow: bool,
    ) -> anyhow::Result<Vec<String>> {
        // Vector to store results
        let mut results = Vec::new();

        let limit = limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .min(DEFAULT_MAX_LIST_LIMIT);

        // TODO: make this more performant than split and allocations?

        let mut threshold = cursor
            .map(|cursor| {
                // Removing leading forward slashes
                let mut file_or_directory = cursor.trim_start_matches('/');

                if cursor.starts_with("pubky://") {
                    file_or_directory = cursor
                        .split(entry_path.as_str())
                        .last()
                        .expect("should not be reachable")
                };

                next_threshold(
                    entry_path.as_str(),
                    file_or_directory,
                    file_or_directory.ends_with('/'),
                    reverse,
                    shallow,
                )
            })
            .unwrap_or(next_threshold(
                entry_path.as_str(),
                "",
                false,
                reverse,
                shallow,
            ));

        for _ in 0..limit {
            if let Some((key, _)) = if reverse {
                self.tables.entries.get_lower_than(txn, &threshold)?
            } else {
                self.tables.entries.get_greater_than(txn, &threshold)?
            } {
                if !key.starts_with(entry_path.as_str()) {
                    break;
                }

                if shallow {
                    let mut split = key[entry_path.as_str().len()..].split('/');
                    let file_or_directory = split.next().expect("should not be reachable");

                    let is_directory = split.next().is_some();

                    threshold = next_threshold(
                        entry_path.as_str(),
                        file_or_directory,
                        is_directory,
                        reverse,
                        shallow,
                    );

                    results.push(format!(
                        "pubky://{}{file_or_directory}{}",
                        entry_path.as_str(),
                        if is_directory { "/" } else { "" }
                    ));
                } else {
                    threshold = key.to_string();
                    results.push(format!("pubky://{}", key))
                }
            };
        }

        Ok(results)
    }
}

/// Calculate the next threshold
#[instrument]
fn next_threshold(
    path: &str,
    file_or_directory: &str,
    is_directory: bool,
    reverse: bool,
    shallow: bool,
) -> String {
    format!(
        "{path}{file_or_directory}{}",
        if file_or_directory.is_empty() {
            // No file_or_directory, early return
            if reverse {
                // `path/to/dir/\x7f` to catch all paths than `path/to/dir/`
                "\x7f"
            } else {
                ""
            }
        } else if shallow & is_directory {
            if reverse {
                // threshold = `path/to/dir\x2e`, since `\x2e` is lower   than `/`
                "\x2e"
            } else {
                //threshold = `path/to/dir\x7f`, since `\x7f` is greater than `/`
                "\x7f"
            }
        } else {
            ""
        }
    )
}

/// The location of the file.
/// This is used to determine where the file is stored.
/// Used during the transition process from LMDB to OpenDAL.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum FileLocation {
    LMDB,
    OpenDal,
}

impl Default for FileLocation {
    fn default() -> Self {
        FileLocation::LMDB
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Entry {
    /// Encoding version
    version: usize,
    /// Modified at
    timestamp: Timestamp,
    content_hash: EntryHash,
    content_length: usize,
    content_type: String,
    file_location: FileLocation, // TODO: Migrate LMDB to add this field.
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryHash(Hash);

impl Default for EntryHash {
    fn default() -> Self {
        Self(Hash::from_bytes([0; 32]))
    }
}

impl Serialize for EntryHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = self.0.as_bytes();
        bytes.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EntryHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: [u8; 32] = Deserialize::deserialize(deserializer)?;
        Ok(Self(Hash::from_bytes(bytes)))
    }
}

impl Entry {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn chunk_key(&self, chunk_index: u32) -> [u8; 12] {
        let mut chunk_key = [0; 12];
        chunk_key[0..8].copy_from_slice(&self.timestamp.to_bytes());
        chunk_key[8..].copy_from_slice(&chunk_index.to_be_bytes());
        chunk_key
    }

    // === Setters ===

    pub fn set_timestamp(&mut self, timestamp: &Timestamp) -> &mut Self {
        self.timestamp = *timestamp;
        self
    }

    pub fn set_content_hash(&mut self, content_hash: Hash) -> &mut Self {
        EntryHash(content_hash).clone_into(&mut self.content_hash);
        self
    }

    pub fn set_content_length(&mut self, content_length: usize) -> &mut Self {
        self.content_length = content_length;
        self
    }

    pub fn set_file_location(&mut self, file_location: FileLocation) -> &mut Self {
        self.file_location = file_location;
        self
    }

    // === Getters ===

    pub fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    pub fn content_hash(&self) -> &Hash {
        &self.content_hash.0
    }

    pub fn content_length(&self) -> usize {
        self.content_length
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn file_location(&self) -> &FileLocation {
        &self.file_location
    }

    pub fn file_id(&self) -> InDbFileId {
        InDbFileId::from(self.timestamp)
    }

    // === Public Method ===

    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("Session::serialize")
    }

    pub fn deserialize(bytes: &[u8]) -> core::result::Result<Self, postcard::Error> {
        if bytes[0] > 0 {
            panic!("Unknown Entry version");
        }

        from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::LmDB;
    use crate::{
        persistence::{files::FileIoError, lmdb::tables::files::{InDbTempFile, SyncInDbTempFileWriter}},
        shared::webdav::{EntryPath, WebDavPath},
    };
    use bytes::Bytes;
    use pkarr::Keypair;
    use std::io::Read;

    #[tokio::test]
    async fn test_write_read_delete_method() {
        let mut db = LmDB::test();

        let path = EntryPath::new(
            Keypair::random().public_key(),
            WebDavPath::new("/pub/foo.txt").unwrap(),
        );
        let file = InDbTempFile::zeros(5).await.unwrap();
        let entry = db.write_entry_from_file_sync(&path, &file).unwrap();

        let read_entry = db.get_entry(&path).unwrap();
        assert_eq!(entry.content_hash(), read_entry.content_hash());
        assert_eq!(entry.content_length(), read_entry.content_length());
        assert_eq!(entry.timestamp(), read_entry.timestamp());

        let read_file = db.read_file(&entry.file_id()).await.unwrap();
        let mut file_handle = read_file.open_file_handle().unwrap();
        let mut content = vec![];
        file_handle.read_to_end(&mut content).unwrap();
        assert_eq!(content, vec![0, 0, 0, 0, 0]);

        db.delete_entry_and_file_sync(&path).unwrap();


        // Verify the entry and file are deleted
        assert!(!db.entry_exists(&path).unwrap(), "File should be deleted");
    }

    #[tokio::test]
    async fn entries() -> anyhow::Result<()> {
        let mut db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";

        let entry_path = EntryPath::new(public_key, WebDavPath::new(path).unwrap());
        let chunk = Bytes::from(vec![1, 2, 3, 4, 5]);
        let mut writer = SyncInDbTempFileWriter::new()?;
        writer.write_chunk(&chunk)?;
        let file = writer.complete()?;

        db.write_entry_from_file_sync(&entry_path, &file)?;

        let entry = db.get_entry(&entry_path).unwrap();

        assert_eq!(
            entry.content_hash(),
            &[
                2, 79, 103, 192, 66, 90, 61, 192, 47, 186, 245, 140, 185, 61, 229, 19, 46, 61, 117,
                197, 25, 250, 160, 186, 218, 33, 73, 29, 136, 201, 112, 87
            ]
        );

        let read_file = db.read_file(&entry.file_id()).await.unwrap();
        let mut file_handle = read_file.open_file_handle().unwrap();
        let mut content = vec![];
        file_handle.read_to_end(&mut content).unwrap();
        assert_eq!(content, vec![1, 2, 3, 4, 5]);
        Ok(())
    }

    #[tokio::test]
    async fn chunked_entry() -> anyhow::Result<()> {
        let mut db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";
        let entry_path = EntryPath::new(public_key, WebDavPath::new(path).unwrap());

        let chunk = Bytes::from(vec![0; 1024 * 1024]);

        let mut writer = SyncInDbTempFileWriter::new()?;
        writer.write_chunk(&chunk)?;
        let file = writer.complete()?;

        db.write_entry_from_file_sync(&entry_path, &file)?;

        let entry = db.get_entry(&entry_path).unwrap();

        assert_eq!(
            entry.content_hash(),
            &[
                72, 141, 226, 2, 247, 59, 217, 118, 222, 78, 112, 72, 244, 225, 243, 154, 119, 109,
                134, 213, 130, 183, 52, 143, 245, 59, 244, 50, 185, 135, 252, 168
            ]
        );

        let read_file = db.read_file(&entry.file_id()).await.unwrap();
        let mut file_handle = read_file.open_file_handle().unwrap();
        let mut content = vec![];
        file_handle.read_to_end(&mut content).unwrap();
        assert_eq!(content, vec![0; 1024 * 1024]);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_entry_not_found() {
        let db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/nonexistent.txt";

        let entry_path = EntryPath::new(public_key, WebDavPath::new(path).unwrap());

        assert!(!db.entry_exists(&entry_path).unwrap(), "File should be deleted");
    }
}
