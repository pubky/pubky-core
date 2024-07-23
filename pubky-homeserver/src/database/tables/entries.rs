use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, time::SystemTime};

use heed::{
    types::{Bytes, Str},
    BoxedError, BytesDecode, BytesEncode, Database,
};

use pubky_common::crypto::Hash;

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Hash, Entry>;

pub const ENTRIES_TABLE: &str = "entries";

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
}
