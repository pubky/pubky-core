use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, time::SystemTime};

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
pub type EntriesTable = Database<Bytes, Bytes>;

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

        self.tables.blobs.put(&mut wtxn, hash.as_bytes(), &bytes)?;

        let mut entry = Entry::new();

        entry.set_content_hash(hash);
        entry.set_content_length(length);

        let mut key = vec![];
        key.extend_from_slice(public_key.as_bytes());
        key.extend_from_slice(path.as_bytes());

        self.tables.entries.put(&mut wtxn, &key, &entry.serialize());

        wtxn.commit()?;

        Ok(())
    }

    pub fn delete_entry(&mut self, public_key: &PublicKey, path: &str) -> anyhow::Result<bool> {
        let mut wtxn = self.env.write_txn()?;

        let mut key = vec![];
        key.extend_from_slice(public_key.as_bytes());
        key.extend_from_slice(path.as_bytes());

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
