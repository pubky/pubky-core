use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use heed::{
    types::{Bytes, Str},
    Database, RoTxn,
};

use pubky_common::{
    crypto::{Hash, Hasher},
    timestamp::Timestamp,
};

use crate::database::DB;

use super::events::Event;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;

pub const ENTRIES_TABLE: &str = "entries";

impl DB {
    pub fn put_entry(
        &mut self,
        public_key: &PublicKey,
        path: &str,
        rx: flume::Receiver<bytes::Bytes>,
    ) -> anyhow::Result<()> {
        let mut wtxn = self.env.write_txn()?;

        let mut hasher = Hasher::new();
        let mut bytes = vec![];
        let mut length = 0;

        while let Ok(chunk) = rx.recv() {
            hasher.update(&chunk);
            bytes.extend_from_slice(&chunk);
            length += chunk.len();
        }

        let hash = hasher.finalize();

        let key = hash.as_bytes();

        let mut bytes_with_ref_count = Vec::with_capacity(bytes.len() + 8);
        bytes_with_ref_count.extend_from_slice(&u64::to_be_bytes(0));
        bytes_with_ref_count.extend_from_slice(&bytes);

        // TODO: For now, we set the first 8 bytes to a reference counter
        let exists = self
            .tables
            .blobs
            .get(&wtxn, key)?
            .unwrap_or(bytes_with_ref_count.as_slice());

        let new_count = u64::from_be_bytes(exists[0..8].try_into().unwrap()) + 1;

        bytes_with_ref_count[0..8].copy_from_slice(&u64::to_be_bytes(new_count));

        self.tables
            .blobs
            .put(&mut wtxn, hash.as_bytes(), &bytes_with_ref_count)?;

        let mut entry = Entry::new();

        entry.set_content_hash(hash);
        entry.set_content_length(length);

        let key = format!("{public_key}/{path}");

        self.tables
            .entries
            .put(&mut wtxn, &key, &entry.serialize())?;

        if path.starts_with("pub/") {
            let url = format!("pubky://{key}");
            let event = Event::put(&url);
            let value = event.serialize();

            let key = entry.timestamp.to_string();

            self.tables.events.put(&mut wtxn, &key, &value)?;

            // TODO: delete older events.
            // TODO: move to events.rs
        }

        wtxn.commit()?;

        Ok(())
    }

    pub fn delete_entry(&mut self, public_key: &PublicKey, path: &str) -> anyhow::Result<bool> {
        let mut wtxn = self.env.write_txn()?;

        let key = format!("{public_key}/{path}");

        let deleted = if let Some(bytes) = self.tables.entries.get(&wtxn, &key)? {
            let entry = Entry::deserialize(bytes)?;

            let mut bytes_with_ref_count = self
                .tables
                .blobs
                .get(&wtxn, entry.content_hash())?
                .map_or(vec![], |s| s.to_vec());

            let arr: [u8; 8] = bytes_with_ref_count[0..8].try_into().unwrap_or([0; 8]);
            let reference_count = u64::from_be_bytes(arr);

            let deleted_blobs = if reference_count > 1 {
                // decrement reference count

                bytes_with_ref_count[0..8].copy_from_slice(&(reference_count - 1).to_be_bytes());

                self.tables
                    .blobs
                    .put(&mut wtxn, entry.content_hash(), &bytes_with_ref_count)?;

                true
            } else {
                self.tables.blobs.delete(&mut wtxn, entry.content_hash())?
            };

            let deleted_entry = self.tables.entries.delete(&mut wtxn, &key)?;

            // create DELETE event
            if path.starts_with("pub/") {
                let url = format!("pubky://{key}");

                let event = Event::delete(&url);
                let value = event.serialize();

                let key = Timestamp::now().to_string();

                self.tables.events.put(&mut wtxn, &key, &value)?;

                // TODO: delete older events.
                // TODO: move to events.rs
            }

            deleted_entry && deleted_blobs
        } else {
            false
        };

        wtxn.commit()?;

        Ok(deleted)
    }

    pub fn contains_directory(&self, txn: &RoTxn, path: &str) -> anyhow::Result<bool> {
        Ok(self.tables.entries.get_greater_than(txn, path)?.is_some())
    }

    /// Return a list of pubky urls.
    ///
    /// - limit defaults to [Config::default_list_limit] and capped by [Config::max_list_limit]
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
            .unwrap_or(self.config.default_list_limit())
            .min(self.config.max_list_limit());

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
    content_hash: [u8; 32],
    content_length: usize,
    content_type: String,
    // user_metadata: ?
}

// TODO: get headers like Etag

impl Entry {
    pub fn new() -> Self {
        Default::default()
    }

    // === Setters ===

    pub fn set_content_hash(&mut self, content_hash: Hash) -> &mut Self {
        content_hash.as_bytes().clone_into(&mut self.content_hash);
        self
    }

    pub fn set_content_length(&mut self, content_length: usize) -> &mut Self {
        self.content_length = content_length;
        self
    }

    // === Getters ===

    pub fn content_hash(&self) -> &[u8; 32] {
        &self.content_hash
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
