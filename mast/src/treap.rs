use blake3::{Hash, Hasher};

use std::cmp::{self, Ordering};
use std::collections::HashMap;
use std::mem;
use std::ops::Deref;

use crate::storage::memory::MemoryStorage;

const EMPTY_HASH: Hash = Hash::from_bytes([0_u8; 32]);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub(crate) key: Box<[u8]>,
    pub(crate) value: Hash,
    pub(crate) rank: Hash,
    pub(crate) left: Hash,
    pub(crate) right: Hash,
}

impl Node {
    fn new(key: &[u8], value: Hash) -> Self {
        let mut hasher = Hasher::new();
        hasher.update(key);

        let rank = hasher.finalize();

        Self {
            key: key.into(),
            value,
            left: EMPTY_HASH,
            right: EMPTY_HASH,
            rank,
        }
    }

    /// Returns the hash of the node.
    pub fn hash(&self) -> Hash {
        let mut hasher = Hasher::new();

        hasher.update(&self.key);
        hasher.update(self.value.as_bytes());
        hasher.update(self.left.as_bytes());
        hasher.update(self.right.as_bytes());

        hasher.finalize()
    }

    fn to_bytes(&self) -> Box<[u8]> {
        let mut bytes = vec![];

        bytes.extend_from_slice(self.value.as_bytes());
        bytes.extend_from_slice(self.left.as_bytes());
        bytes.extend_from_slice(self.right.as_bytes());
        bytes.extend_from_slice(&self.key);

        bytes.into_boxed_slice()
    }

    fn from_bytes(bytes: &Box<[u8]>) -> Self {
        // TODO: Make sure that bytes is long enough at least >96 bytes.

        let mut node = Self::new(
            &bytes[96..],
            Hash::from_bytes(bytes[..32].try_into().unwrap()),
        );

        node.left = Hash::from_bytes(bytes[32..64].try_into().unwrap());
        node.right = Hash::from_bytes(bytes[64..96].try_into().unwrap());

        node
    }

    fn set_left(&mut self, left: Hash, storage: &mut MemoryStorage) {}
}

#[derive(Debug)]
pub struct Treap {
    pub(crate) root: Hash,
    storage: MemoryStorage,
}

impl Treap {
    pub fn new(storage: MemoryStorage) -> Self {
        Self {
            root: EMPTY_HASH,
            storage,
        }
    }

    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        let value = self.insert_blob(value);
        let mut node = Node::new(key, value);

        // TODO: batch inserting updated nodes.

        let new_root = self.insert_impl(&mut node, self.root);
        self.root = new_root.hash();
    }

    // Recursive insertion (unzipping) algorithm.
    //
    // Returns the new root node.
    fn insert_impl(&mut self, x: &mut Node, root_hash: Hash) -> Node {
        if let Some(mut root) = self.get_node(root_hash) {
            if x.key < root.key {
                if self.insert_impl(x, root.left).key == x.key {
                    if x.rank.as_bytes() < root.rank.as_bytes() {
                        root.left = self.store_node(x);
                        self.store_node(&root);
                    } else {
                        root.left = x.right;
                        x.right = self.store_node(&root);

                        self.store_node(x);
                        return x.clone();
                    }
                }
            } else {
                if self.insert_impl(x, root.right).key == x.key {
                    if x.rank.as_bytes() < root.rank.as_bytes() {
                        root.right = self.store_node(x);

                        self.store_node(&root);
                    } else {
                        root.right = x.left;
                        x.right = self.store_node(&root);

                        self.store_node(x);

                        return x.clone();
                    }
                }
            }

            self.store_node(&root);

            return root;
        } else {
            self.store_node(x);

            return x.clone();
        }
    }

    /// Store a node after it has been modified and had a new hash.
    fn store_node(&mut self, node: &Node) -> Hash {
        // TODO: save the hash somewhere in the Node instead of hashing it again.

        let hash = node.hash();
        self.storage.insert_node(node);

        hash
    }

    // TODO: Add stream input API.
    fn insert_blob(&mut self, blob: &[u8]) -> Hash {
        let mut hasher = Hasher::new();
        hasher.update(blob);
        let hash = hasher.finalize();

        self.storage.insert_blob(hash, blob.into());

        hash
    }

    // TODO: move to storage abstraction.
    pub(crate) fn get_node(&self, hash: Hash) -> Option<Node> {
        self.storage.get_node(&hash).cloned()
    }
}

impl Default for Treap {
    fn default() -> Self {
        Self::new(MemoryStorage::new())
    }
}
