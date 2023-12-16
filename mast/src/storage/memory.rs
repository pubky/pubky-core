use blake3::Hash;
use std::collections::HashMap;

use crate::treap::Node;

#[derive(Debug)]
pub struct MemoryStorage {
    nodes: HashMap<Hash, Node>,
    blobs: HashMap<Hash, Box<[u8]>>,
}

impl MemoryStorage {
    pub(crate) fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            blobs: HashMap::new(),
        }
    }

    pub(crate) fn insert_node(&mut self, node: &Node) {
        self.nodes.insert(node.hash(), node.clone());
    }

    pub(crate) fn insert_blob(&mut self, hash: Hash, blob: &[u8]) {
        self.blobs.insert(hash, blob.into());
    }

    pub(crate) fn get_node(&self, hash: &Hash) -> Option<&Node> {
        self.nodes.get(hash)
    }
}
