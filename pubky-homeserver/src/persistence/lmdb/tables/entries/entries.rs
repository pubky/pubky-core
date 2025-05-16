use super::super::events::Event;
use super::{super::super::LmDB, EntryPath, InDbFileId, InDbTempFile};
use crate::constants::{DEFAULT_LIST_LIMIT, DEFAULT_MAX_LIST_LIMIT};
use heed::{
    types::{Bytes, Str},
    Database, RoTxn,
};
use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use pubky_common::{
    crypto::{Hash, Hasher},
    timestamp::Timestamp,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tracing::instrument;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;

pub const ENTRIES_TABLE: &str = "entries";

impl LmDB {
    /// Write an entry by an author at a given path.
    ///
    /// The path has to start with a forward slash `/`
    #[cfg(test)]
    pub fn create_entry_writer(
        &mut self,
        public_key: &PublicKey,
        path: &str,
    ) -> anyhow::Result<EntryWriter> {
        EntryWriter::new(self, public_key, path)
    }

    /// Writes an entry to the database.
    ///
    /// The entry is written to the database and the file is written to the blob store.
    /// An event is written to the events table.
    /// The entry is returned.
    pub async fn write_entry2(
        &mut self,
        path: &EntryPath,
        file: &InDbTempFile,
    ) -> anyhow::Result<Entry> {
        let mut db = self.clone();
        let path = path.clone();
        let file = file.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Entry> {
            db.write_entry2_sync(&path, &file)
        })
        .await?
    }

    /// Writes an entry to the database.
    ///
    /// The entry is written to the database and the file is written to the blob store.
    /// An event is written to the events table.
    /// The entry is returned.
    pub fn write_entry2_sync(
        &mut self,
        path: &EntryPath,
        file: &InDbTempFile,
    ) -> anyhow::Result<Entry> {
        let mut wtxn = self.env.write_txn()?;
        let mut entry = Entry::new();
        entry.set_content_hash(*file.hash());
        entry.set_content_length(file.len());
        let file_id = self.write_file_sync(&file, &mut wtxn)?;
        entry.set_timestamp(file_id.timestamp());
        let entry_key = path.key();
        self.tables
            .entries
            .put(&mut wtxn, entry_key.as_str(), &entry.serialize())?;

        // Write a public [Event].
        let url = format!("pubky://{}", entry_key);
        let event = Event::put(&url);
        let value = event.serialize();

        self.tables
            .events
            .put(&mut wtxn, file_id.timestamp().to_string().as_str(), &value)?;
        wtxn.commit()?;
        Ok(entry)
    }

    /// Get an entry from the database.
    /// This doesn't include the file but only metadata.
    pub fn get_entry2(&self, path: &EntryPath) -> anyhow::Result<Option<Entry>> {
        let txn = self.env.read_txn()?;
        let key = path.key();
        let entry = match self.tables.entries.get(&txn, key.as_str())? {
            Some(bytes) => Entry::deserialize(bytes)?,
            None => return Ok(None),
        };
        Ok(Some(entry))
    }

    /// Delete an entry including the associated file from the database.
    pub async fn delete_entry2(&mut self, path: &EntryPath) -> anyhow::Result<bool> {
        let mut db = self.clone();
        let path = path.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            db.delete_entry2_sync(&path)
        })
        .await?
    }

    /// Delete an entry including the associated file from the database.
    pub fn delete_entry2_sync(&mut self, path: &EntryPath) -> anyhow::Result<bool> {
        let entry = match self.get_entry2(path)? {
            Some(entry) => entry,
            None => return Ok(false),
        };

        let mut wtxn = self.env.write_txn()?;
        let deleted = self.delete_file(&entry.file_id(), &mut wtxn)?;
        if !deleted {
            wtxn.abort();
            return Ok(false);
        }

        let key = path.key();
        let deleted = self.tables.entries.delete(&mut wtxn, key.as_str())?;
        if !deleted {
            wtxn.abort();
            return Ok(false);
        }

        // create DELETE event
        let url = format!("pubky://{key}");

        let event = Event::delete(&url);
        let value = event.serialize();

        let key = Timestamp::now().to_string();

        self.tables.events.put(&mut wtxn, &key, &value)?;

        wtxn.commit()?;
        Ok(true)
    }

    /// Delete an entry by an author at a given path.
    ///
    /// The path has to start with a forward slash `/`
    #[cfg(test)]
    pub fn delete_entry(&mut self, public_key: &PublicKey, path: &str) -> anyhow::Result<bool> {
        let mut wtxn = self.env.write_txn()?;

        let key = format!("{public_key}{path}");

        let deleted = if let Some(bytes) = self.tables.entries.get(&wtxn, &key)? {
            let entry = Entry::deserialize(bytes)?;

            let mut deleted_chunks = false;

            {
                let mut iter = self
                    .tables
                    .blobs
                    .prefix_iter_mut(&mut wtxn, &entry.timestamp.to_bytes())?;

                while iter.next().is_some() {
                    unsafe {
                        deleted_chunks = iter.del_current()?;
                    }
                }
            }

            let deleted_entry = self.tables.entries.delete(&mut wtxn, &key)?;

            // create DELETE event
            if path.starts_with("/pub/") {
                let url = format!("pubky://{key}");

                let event = Event::delete(&url);
                let value = event.serialize();

                let key = Timestamp::now().to_string();

                self.tables.events.put(&mut wtxn, &key, &value)?;

                // TODO: delete events older than a threshold.
                // TODO: move to events.rs
            }

            deleted_entry && deleted_chunks
        } else {
            false
        };

        wtxn.commit()?;

        Ok(deleted)
    }

    pub fn get_entry(
        &self,
        txn: &RoTxn,
        public_key: &PublicKey,
        path: &str,
    ) -> anyhow::Result<Option<Entry>> {
        let key = format!("{public_key}{path}");

        if let Some(bytes) = self.tables.entries.get(txn, &key)? {
            return Ok(Some(Entry::deserialize(bytes)?));
        }

        Ok(None)
    }

    /// Bytes stored at `path` for this user (0 if none).
    pub fn get_entry_content_length(&self, path: &EntryPath) -> anyhow::Result<u64> {
        let content_length = self
            .get_entry2(path)?
            .map(|e| e.content_length() as u64)
            .unwrap_or(0);
        Ok(content_length)
    }

    pub fn contains_directory(&self, txn: &RoTxn, path: &str) -> anyhow::Result<bool> {
        Ok(self.tables.entries.get_greater_than(txn, path)?.is_some())
    }

    /// Return a list of pubky urls.
    ///
    /// - limit defaults to [crate::config::DEFAULT_LIST_LIMIT] and capped by [crate::config::DEFAULT_MAX_LIST_LIMIT]
    pub fn list(
        &self,
        txn: &RoTxn,
        path: &str,
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
                    file_or_directory = cursor.split(path).last().expect("should not be reachable")
                };

                next_threshold(
                    path,
                    file_or_directory,
                    file_or_directory.ends_with('/'),
                    reverse,
                    shallow,
                )
            })
            .unwrap_or(next_threshold(path, "", false, reverse, shallow));

        for _ in 0..limit {
            if let Some((key, _)) = if reverse {
                self.tables.entries.get_lower_than(txn, &threshold)?
            } else {
                self.tables.entries.get_greater_than(txn, &threshold)?
            } {
                if !key.starts_with(path) {
                    break;
                }

                if shallow {
                    let mut split = key[path.len()..].split('/');
                    let file_or_directory = split.next().expect("should not be reachable");

                    let is_directory = split.next().is_some();

                    threshold =
                        next_threshold(path, file_or_directory, is_directory, reverse, shallow);

                    results.push(format!(
                        "pubky://{path}{file_or_directory}{}",
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

#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Entry {
    /// Encoding version
    version: usize,
    /// Modified at
    timestamp: Timestamp,
    content_hash: EntryHash,
    content_length: usize,
    content_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EntryHash(Hash);

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

    pub fn file_id(&self) -> InDbFileId {
        InDbFileId::from(self.timestamp)
    }

    // === Public Method ===

    pub fn read_content<'txn>(
        &self,
        db: &'txn LmDB,
        rtxn: &'txn RoTxn,
    ) -> anyhow::Result<impl Iterator<Item = Result<&'txn [u8], heed::Error>> + 'txn> {
        db.read_entry_content(rtxn, self)
    }

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

pub struct EntryWriter<'db> {
    db: &'db LmDB,
    buffer_file: File,
    hasher: Hasher,
    // This directory including the buffer_file is automatically deleted when the EntryWriter is dropped.
    #[allow(dead_code)]
    buffer_dir: tempfile::TempDir,
    entry_key: String,
    timestamp: Timestamp,
}

impl<'db> EntryWriter<'db> {
    pub fn new(db: &'db LmDB, public_key: &PublicKey, path: &str) -> anyhow::Result<Self> {
        let hasher = Hasher::new();
        let timestamp = Timestamp::now();
        let buffer_dir = tempfile::tempdir()?;
        let buffer_path = Self::buffer_file_path(buffer_dir.path());
        let buffer_file = File::create(&buffer_path)?;
        let entry_key = format!("{public_key}{path}");

        Ok(Self {
            db,
            buffer_file,
            hasher,
            buffer_dir,
            entry_key,
            timestamp,
        })
    }

    fn buffer_file_path(buffer_dir: &Path) -> PathBuf {
        buffer_dir.join("buffer.bin")
    }

    /// Same ase [EntryWriter::write_all] but returns a Result of a mutable reference of itself
    /// to enable chaining with [Self::commit].
    pub fn update(&mut self, chunk: &[u8]) -> Result<&mut Self, std::io::Error> {
        self.write_all(chunk)?;

        Ok(self)
    }

    /// Create a chunk key from a timestamp and a chunk index.
    /// Max file size is 2^32 chunks.
    fn create_chunk_key(timestamp: &Timestamp, chunk_index: u32) -> [u8; 12] {
        let mut chunk_key = [0; 12];
        chunk_key[0..8].copy_from_slice(&timestamp.to_bytes());
        chunk_key[8..].copy_from_slice(&chunk_index.to_be_bytes());
        chunk_key
    }

    /// Commit blob from the filesystem buffer to LMDB,
    /// write the [Entry], and commit the write transaction.
    pub fn commit(&mut self) -> anyhow::Result<Entry> {
        let hash = self.hasher.finalize();

        // Prepare file to be read again
        self.buffer_file.flush()?;
        self.buffer_file = File::open(Self::buffer_file_path(&self.buffer_dir.path()))?;

        let mut wtxn = self.db.env.write_txn()?;

        let mut chunk_index: u32 = 0;

        loop {
            let mut chunk = vec![0_u8; self.db.max_chunk_size];
            let bytes_read = self.buffer_file.read(&mut chunk)?;
            let is_end_of_file = bytes_read == 0;
            if is_end_of_file {
                break; // EOF reached
            }

            let chunk_key = Self::create_chunk_key(&self.timestamp, chunk_index);
            self.db
                .tables
                .blobs
                .put(&mut wtxn, &chunk_key, &chunk[..bytes_read])?;

            chunk_index += 1;
        }

        let mut entry = Entry::new();
        entry.set_timestamp(&self.timestamp);
        entry.set_content_hash(hash);

        let length = self.buffer_file.metadata()?.len();
        entry.set_content_length(length as usize);

        self.db
            .tables
            .entries
            .put(&mut wtxn, &self.entry_key, &entry.serialize())?;

        // Write a public [Event].
        let url = format!("pubky://{}", self.entry_key);
        let event = Event::put(&url);
        let value = event.serialize();

        let key = entry.timestamp.to_string();

        self.db.tables.events.put(&mut wtxn, &key, &value)?;

        // TODO: delete events older than a threshold.
        // TODO: move to events.rs

        wtxn.commit()?;

        Ok(entry)
    }
}

impl std::io::Write for EntryWriter<'_> {
    /// Write a chunk to a Filesystem based buffer.
    #[inline]
    fn write(&mut self, chunk: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(chunk);
        self.buffer_file.write_all(chunk)?;

        Ok(chunk.len())
    }

    /// Does not do anything, you need to call [Self::commit]
    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // use std::io::Write;

    use std::io::Read;

    use bytes::Bytes;
    use pkarr::Keypair;

    use crate::{
        persistence::lmdb::tables::entries::{EntryPath, InDbTempFile},
        shared::WebDavPath,
    };

    use super::LmDB;

    #[tokio::test]
    async fn test_write_read_delete_method() {
        let mut db = LmDB::test();

        let path = EntryPath::new(
            Keypair::random().public_key(),
            WebDavPath::new("/pub/foo.txt").unwrap(),
        );
        let file = InDbTempFile::zeroes(5).await.unwrap();
        let entry = db.write_entry2_sync(&path, &file).unwrap();

        let read_entry = db.get_entry2(&path).unwrap().expect("Entry doesn't exist");
        assert_eq!(entry.content_hash(), read_entry.content_hash());
        assert_eq!(entry.content_length(), read_entry.content_length());
        assert_eq!(entry.timestamp(), read_entry.timestamp());

        let read_file = db
            .read_file(&entry.file_id())
            .await
            .unwrap()
            .expect("File not found");
        let mut file_handle = read_file.open_file_handle().unwrap();
        let mut content = vec![];
        file_handle.read_to_end(&mut content).unwrap();
        assert_eq!(content, vec![0, 0, 0, 0, 0]);

        let deleted = db.delete_entry2_sync(&path).unwrap();
        assert!(deleted);

        // Verify the entry and file are deleted
        let read_entry = db.get_entry2(&path).unwrap();
        assert!(read_entry.is_none());
        let read_file = db.read_file(&entry.file_id()).await.unwrap();
        assert!(read_file.is_none());
    }

    #[tokio::test]
    async fn test_read_new_method() {
        let mut db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";
        let chunk = Bytes::from(vec![1, 2, 3, 4, 5]);

        // Write with the old method
        let mut writer = db.create_entry_writer(&public_key, path).unwrap();
        writer.update(&chunk).expect("Failed to write chunk");
        writer.commit().expect("Failed to commit");

        // Check read with the new methods
        let entry_path = EntryPath::new(public_key, WebDavPath::new(path).unwrap());
        let entry = db
            .get_entry2(&entry_path)
            .unwrap()
            .expect("Entry not found");
        assert_eq!(entry.content_hash(), entry.content_hash());

        let file = db
            .read_file(&entry.file_id())
            .await
            .unwrap()
            .expect("File not found");
        assert_eq!(file.hash(), &entry.content_hash.0);
        let mut file_handle = file.open_file_handle().unwrap();
        let mut content = vec![];
        file_handle.read_to_end(&mut content).unwrap();
        assert_eq!(content, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_write_new_method() {
        let mut db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";

        // Write with new method
        let entry_path = EntryPath::new(public_key.clone(), WebDavPath::new(path).unwrap());
        let file = InDbTempFile::zeroes(5).await.unwrap();
        let new_entry = db.write_entry2_sync(&entry_path, &file).unwrap();

        // Check read with the old methods
        let rtxn = db.env.read_txn().unwrap();
        let entry = db.get_entry(&rtxn, &public_key, path).unwrap().unwrap();
        assert_eq!(entry.content_hash(), new_entry.content_hash());
        assert_eq!(entry.content_length(), new_entry.content_length());
        assert_eq!(entry.timestamp(), new_entry.timestamp());
        assert_eq!(
            entry.content_hash().to_hex().as_str(),
            "cdc96eca844d7912acdbb3dca677757d0db5747a1df61166339cfc7156d4880f"
        );
    }

    #[tokio::test]
    async fn entries() -> anyhow::Result<()> {
        let mut db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";

        let chunk = Bytes::from(vec![1, 2, 3, 4, 5]);

        let mut writer = db.create_entry_writer(&public_key, path)?;
        writer.update(&chunk).expect("Failed to write chunk");
        writer.commit().expect("Failed to commit");

        let rtxn = db.env.read_txn().unwrap();
        let entry = db.get_entry(&rtxn, &public_key, path).unwrap().unwrap();

        assert_eq!(
            entry.content_hash(),
            &[
                2, 79, 103, 192, 66, 90, 61, 192, 47, 186, 245, 140, 185, 61, 229, 19, 46, 61, 117,
                197, 25, 250, 160, 186, 218, 33, 73, 29, 136, 201, 112, 87
            ]
        );

        let mut blob = vec![];

        {
            let mut iter = entry.read_content(&db, &rtxn).unwrap();

            while let Some(Ok(chunk)) = iter.next() {
                blob.extend_from_slice(chunk);
            }
        }

        assert_eq!(blob, vec![1, 2, 3, 4, 5]);

        rtxn.commit().unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn chunked_entry() -> anyhow::Result<()> {
        let mut db = LmDB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";

        let chunk = Bytes::from(vec![0; 1024 * 1024]);

        db.create_entry_writer(&public_key, path)?
            .update(&chunk)?
            .commit()?;

        let rtxn = db.env.read_txn().unwrap();
        let entry = db.get_entry(&rtxn, &public_key, path).unwrap().unwrap();

        assert_eq!(
            entry.content_hash(),
            &[
                72, 141, 226, 2, 247, 59, 217, 118, 222, 78, 112, 72, 244, 225, 243, 154, 119, 109,
                134, 213, 130, 183, 52, 143, 245, 59, 244, 50, 185, 135, 252, 168
            ]
        );

        let mut blob = vec![];

        {
            let mut iter = entry.read_content(&db, &rtxn).unwrap();

            while let Some(Ok(chunk)) = iter.next() {
                blob.extend_from_slice(chunk);
            }
        }

        assert_eq!(blob, vec![0; 1024 * 1024]);

        let stats = db.tables.blobs.stat(&rtxn).unwrap();
        assert_eq!(stats.overflow_pages, 0);

        rtxn.commit().unwrap();

        Ok(())
    }
}
