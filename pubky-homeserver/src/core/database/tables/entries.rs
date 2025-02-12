use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};
use tracing::instrument;

use heed::{
    types::{Bytes, Str},
    Database, RoTxn,
};

use pubky_common::{
    crypto::{Hash, Hasher},
    timestamp::Timestamp,
};

use crate::core::database::DB;

use super::events::Event;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;

pub const ENTRIES_TABLE: &str = "entries";

impl DB {
    /// Write an entry by an author at a given path.
    ///
    /// The path has to start with a forward slash `/`
    pub fn write_entry(
        &mut self,
        public_key: &PublicKey,
        path: &str,
    ) -> anyhow::Result<EntryWriter> {
        EntryWriter::new(self, public_key, path)
    }

    /// Delete an entry by an author at a given path.
    ///
    /// The path has to start with a forward slash `/`
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

    pub fn contains_directory(&self, txn: &RoTxn, path: &str) -> anyhow::Result<bool> {
        Ok(self.tables.entries.get_greater_than(txn, path)?.is_some())
    }

    /// Return a list of pubky urls.
    ///
    /// - limit defaults to [crate::core::Config::default_list_limit] and capped by [crate::core::Config::max_list_limit]
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
            .unwrap_or(self.config().default_list_limit)
            .min(self.config().max_list_limit);

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
    // user_metadata: ?
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

    // === Public Method ===

    pub fn read_content<'txn>(
        &self,
        db: &'txn DB,
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
    db: &'db DB,
    buffer: File,
    hasher: Hasher,
    buffer_path: PathBuf,
    entry_key: String,
    timestamp: Timestamp,
    is_public: bool,
}

impl<'db> EntryWriter<'db> {
    pub fn new(db: &'db DB, public_key: &PublicKey, path: &str) -> anyhow::Result<Self> {
        let hasher = Hasher::new();

        let timestamp = Timestamp::now();

        let buffer_path = db.buffers_dir.join(timestamp.to_string());

        let buffer = File::create(&buffer_path)?;

        let entry_key = format!("{public_key}{path}");

        Ok(Self {
            db,
            buffer,
            hasher,
            buffer_path,
            entry_key,
            timestamp,
            is_public: path.starts_with("/pub/"),
        })
    }

    /// Same ase [EntryWriter::write_all] but returns a Result of a mutable reference of itself
    /// to enable chaining with [Self::commit].
    pub fn update(&mut self, chunk: &[u8]) -> Result<&mut Self, std::io::Error> {
        self.write_all(chunk)?;

        Ok(self)
    }

    /// Commit blob from the filesystem buffer to LMDB,
    /// write the [Entry], and commit the write transaction.
    pub fn commit(&self) -> anyhow::Result<Entry> {
        let hash = self.hasher.finalize();

        let mut buffer = File::open(&self.buffer_path)?;

        let mut wtxn = self.db.env.write_txn()?;

        let mut chunk_key = [0; 12];
        chunk_key[0..8].copy_from_slice(&self.timestamp.to_bytes());

        let mut chunk_index: u32 = 0;

        loop {
            let mut chunk = vec![0_u8; self.db.max_chunk_size];

            let bytes_read = buffer.read(&mut chunk)?;

            if bytes_read == 0 {
                break; // EOF reached
            }

            chunk_key[8..].copy_from_slice(&chunk_index.to_be_bytes());

            self.db
                .tables
                .blobs
                .put(&mut wtxn, &chunk_key, &chunk[..bytes_read])?;

            chunk_index += 1;
        }

        let mut entry = Entry::new();
        entry.set_timestamp(&self.timestamp);

        entry.set_content_hash(hash);

        let length = buffer.metadata()?.len();
        entry.set_content_length(length as usize);

        self.db
            .tables
            .entries
            .put(&mut wtxn, &self.entry_key, &entry.serialize())?;

        // Write a public [Event].
        if self.is_public {
            let url = format!("pubky://{}", self.entry_key);
            let event = Event::put(&url);
            let value = event.serialize();

            let key = entry.timestamp.to_string();

            self.db.tables.events.put(&mut wtxn, &key, &value)?;

            // TODO: delete events older than a threshold.
            // TODO: move to events.rs
        }

        wtxn.commit()?;

        std::fs::remove_file(&self.buffer_path)?;

        Ok(entry)
    }
}

impl<'db> std::io::Write for EntryWriter<'db> {
    /// Write a chunk to a Filesystem based buffer.
    #[inline]
    fn write(&mut self, chunk: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(chunk);
        self.buffer.write_all(chunk)?;

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
    use bytes::Bytes;
    use pkarr::Keypair;

    use super::DB;

    #[tokio::test]
    async fn entries() -> anyhow::Result<()> {
        let mut db = DB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";

        let chunk = Bytes::from(vec![1, 2, 3, 4, 5]);

        db.write_entry(&public_key, path)?
            .update(&chunk)?
            .commit()?;

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
                blob.extend_from_slice(&chunk);
            }
        }

        assert_eq!(blob, vec![1, 2, 3, 4, 5]);

        rtxn.commit().unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn chunked_entry() -> anyhow::Result<()> {
        let mut db = DB::test();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let path = "/pub/foo.txt";

        let chunk = Bytes::from(vec![0; 1024 * 1024]);

        db.write_entry(&public_key, path)?
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
                blob.extend_from_slice(&chunk);
            }
        }

        assert_eq!(blob, vec![0; 1024 * 1024]);

        let stats = db.tables.blobs.stat(&rtxn).unwrap();
        assert_eq!(stats.overflow_pages, 0);

        rtxn.commit().unwrap();

        Ok(())
    }
}
