use heed::{
    types::{Bytes, Str},
    Database,
};
use postcard::from_bytes;
use pubky_common::{crypto::Hash, timestamp::Timestamp};
use serde::{Deserialize, Serialize};

/// full_path(pubky/*path) => Entry.
pub type EntriesTable = Database<Str, Bytes>;
pub const ENTRIES_TABLE: &str = "entries";

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
    #[cfg(test)]
    pub fn new() -> Self {
        Default::default()
    }

    // === Setters ===

    #[cfg(test)]
    pub fn set_timestamp(&mut self, timestamp: &Timestamp) -> &mut Self {
        self.timestamp = *timestamp;
        self
    }

    #[cfg(test)]
    pub fn set_content_hash(&mut self, content_hash: Hash) -> &mut Self {
        EntryHash(content_hash).clone_into(&mut self.content_hash);
        self
    }

    #[cfg(test)]
    pub fn set_content_length(&mut self, content_length: usize) -> &mut Self {
        self.content_length = content_length;
        self
    }

    #[cfg(test)]
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

    #[cfg(test)]
    pub fn serialize(&self) -> Vec<u8> {
        use postcard::to_allocvec;
        to_allocvec(self).expect("SessionInfo::serialize")
    }

    pub fn deserialize(bytes: &[u8]) -> core::result::Result<Self, postcard::Error> {
        if bytes[0] > 0 {
            panic!("Unknown Entry version");
        }

        from_bytes(bytes)
    }
}
