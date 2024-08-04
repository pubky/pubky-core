use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt::Result, time::SystemTime};

use heed::{
    types::{Bytes, Str},
    BoxedError, BytesDecode, BytesEncode, Database,
};

use pubky_common::{
    crypto::{Hash, Hasher},
    timestamp::Timestamp,
};

use crate::database::DB;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;

pub const ENTRIES_TABLE: &str = "entries";

const MAX_LIST_LIMIT: i32 = 100;

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

        self.tables.blobs.put(&mut wtxn, hash.as_bytes(), &bytes)?;

        let mut entry = Entry::new();

        entry.set_content_hash(hash);
        entry.set_content_length(length);

        let key = format!("{public_key}/{path}");

        self.tables.entries.put(&mut wtxn, &key, &entry.serialize());

        wtxn.commit()?;

        Ok(())
    }

    pub fn delete_entry(&mut self, public_key: &PublicKey, path: &str) -> anyhow::Result<bool> {
        let mut wtxn = self.env.write_txn()?;

        let key = format!("{public_key}/{path}");

        let deleted = if let Some(bytes) = self.tables.entries.get(&wtxn, &key)? {
            let entry = Entry::deserialize(bytes)?;

            // TODO: reference counting of blobs
            let deleted_blobs = self.tables.blobs.delete(&mut wtxn, entry.content_hash())?;

            let deleted_entry = self.tables.entries.delete(&mut wtxn, &key)?;

            deleted_entry & deleted_blobs
        } else {
            false
        };

        wtxn.commit()?;

        Ok(deleted)
    }

    /// Return a list of pubky urls.
    ///
    /// - limit defaults to and capped by [MAX_LIST_LIMIT]
    pub fn list(
        &self,
        public_key: &PublicKey,
        prefix: &str,
        reverse: bool,
        limit: Option<i32>,
    ) -> anyhow::Result<Vec<String>> {
        let db = self.tables.entries;
        let txn = self.env.read_txn()?;

        let prefix = format!("{public_key}/{prefix}");
        let limit = limit.unwrap_or(MAX_LIST_LIMIT).min(MAX_LIST_LIMIT);

        // Vector to store results
        let mut results = Vec::new();

        // Fetch data based on direction
        if reverse {
            let mut iter = self.tables.entries.rev_prefix_iter(&txn, &prefix)?;

            for _ in 0..limit {
                if let Some((key, _)) = iter.next().transpose()? {
                    results.push(format!("pubky://{}", key))
                };
            }
        } else {
            let mut iter = self.tables.entries.prefix_iter(&txn, &prefix)?;

            for _ in 0..limit {
                if let Some((key, _)) = iter.next().transpose()? {
                    results.push(format!("pubky://{}", key))
                };
            }
        }

        Ok(results)
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Entry {
    /// Encoding version
    version: usize,
    /// Modified at
    timestamp: u64,
    content_hash: [u8; 32],
    content_length: usize,
    content_type: String,
    // user_metadata: ?
}

// TODO: get headers like Etag

impl Entry {
    pub fn new() -> Self {
        Self {
            timestamp: Timestamp::now().into_inner(),
            ..Default::default()
        }
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

    pub fn set_content_type(&mut self, content_type: &str) -> &mut Self {
        self.content_type = content_type.to_string();
        self
    }

    // === Getters ===

    pub fn content_hash(&self) -> &[u8; 32] {
        &self.content_hash
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
