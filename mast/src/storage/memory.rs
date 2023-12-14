//! In memory Mast storage.

use blake3::{Hash, Hasher};
use bytes::{Bytes, BytesMut};
use std::collections::HashMap;

// TODO: storage abstraction.
#[derive(Debug)]
pub struct Storage {
    storage: HashMap<Hash, Bytes>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            storage: HashMap::default(),
        }
    }

    pub fn get(&self, hash: &Hash) -> Option<Bytes> {
        self.storage.get(hash).cloned()
    }

    pub fn insert_bytes(&mut self, bytes: Bytes) -> Hash {
        let hash = Hasher::new().update(&bytes).finalize();
        // TODO: should I add a prefix here?
        self.storage.insert(hash, bytes);
        hash
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}
