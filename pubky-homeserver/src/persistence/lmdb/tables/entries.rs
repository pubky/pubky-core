use super::super::LmDB;
use crate::constants::{DEFAULT_LIST_LIMIT, DEFAULT_MAX_LIST_LIMIT};
use crate::shared::webdav::EntryPath;
use heed::{
    types::{Bytes, Str},
    Database, RoTxn,
};
use postcard::{from_bytes, to_allocvec};
use pubky_common::{crypto::Hash, timestamp::Timestamp};
use serde::{Deserialize, Serialize};
use tracing::instrument;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;
pub const ENTRIES_TABLE: &str = "entries";

impl LmDB {
    // /// Get an entry from the database.
    // /// This doesn't include the file but only metadata.
    // pub fn get_entry(&self, path: &EntryPath) -> Result<Entry, FileIoError> {
    //     let txn = self.env.read_txn()?;
    //     let entry = match self.tables.entries.get(&txn, path.as_str())? {
    //         Some(bytes) => Entry::deserialize(bytes)?,
    //         None => return Err(FileIoError::NotFound),
    //     };
    //     Ok(entry)
    // }

    // /// Bytes stored at `path`.
    // /// Fails if the entry does not exist.
    // pub fn get_entry_content_length(&self, path: &EntryPath) -> Result<u64, FileIoError> {
    //     let content_length = self.get_entry(path)?.content_length() as u64;
    //     Ok(content_length)
    // }

    // /// Bytes stored at `path`.
    // /// Returns 0 if the entry does not exist.
    // pub fn get_entry_content_length_default_zero(
    //     &self,
    //     path: &EntryPath,
    // ) -> Result<u64, FileIoError> {
    //     match self.get_entry_content_length(path) {
    //         Ok(length) => Ok(length),
    //         Err(FileIoError::NotFound) => Ok(0),
    //         Err(e) => Err(e),
    //     }
    // }

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

    pub fn set_content_type(&mut self, content_type: String) -> &mut Self {
        self.content_type = content_type;
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
