use blake3::Hash;
use std::collections::HashMap;

use crate::Node;

#[derive(Debug)]
pub struct MemoryStorage {
    roots: HashMap<Box<[u8]>, Node>,
    nodes: HashMap<Hash, Node>,
    blobs: HashMap<Hash, Box<[u8]>>,
}

impl MemoryStorage {
    pub(crate) fn new() -> Self {
        Self {
            roots: HashMap::new(),
            nodes: HashMap::new(),
            blobs: HashMap::new(),
        }
    }

    // TODO: return result or something.

    pub(crate) fn insert_root(&mut self, name: &[u8], node: Node) {
        self.roots.insert(name.into(), node);
    }

    pub(crate) fn insert_node(&mut self, node: &Node) {
        self.nodes.insert(*node.hash(), node.clone());
    }

    pub(crate) fn insert_blob(&mut self, hash: Hash, blob: &[u8]) {
        self.blobs.insert(hash, blob.into());
    }

    pub(crate) fn get_node(&self, hash: &Hash) -> Option<Node> {
        self.nodes.get(hash).cloned()
    }
}
