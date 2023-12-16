#![allow(unused)]

use blake3::{Hash, Hasher};

use std::cmp::{self, Ordering};
use std::collections::HashMap;
use std::mem;
use std::ops::Deref;

const EMPTY_HASH: Hash = Hash::from_bytes([0_u8; 32]);

#[derive(Debug, Clone, PartialEq)]
struct Node {
    key: Box<[u8]>,
    value: Hash,
    rank: Hash,
    left: Hash,
    right: Hash,
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

    // TODO: memoize
    fn hash(&self) -> Hash {
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
}

#[derive(Debug)]
pub struct Treap {
    root: Hash,
    storage: HashMap<Hash, Box<[u8]>>,
}

impl Treap {
    pub fn new(storage: HashMap<Hash, Box<[u8]>>) -> Self {
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
        dbg!(("new root", self.root));
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
                dbg!("going right",);
                if self.insert_impl(x, root.right).key == x.key {
                    if x.rank.as_bytes() < root.rank.as_bytes() {
                        root.right = self.store_node(x);

                        self.store_node(&root);
                    } else {
                        root.right = x.left;
                        x.right = self.store_node(&root);

                        self.store_node(x);

                        // dbg!(("after going right", &x, &root));
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
        self.storage.insert(hash, node.to_bytes());

        hash
    }

    // TODO: Add stream input API.
    fn insert_blob(&mut self, blob: &[u8]) -> Hash {
        let mut hasher = Hasher::new();
        hasher.update(blob);
        let hash = hasher.finalize();

        self.storage.insert(hash, blob.into());

        hash
    }

    // TODO: move to storage abstraction.
    fn get_node(&self, hash: Hash) -> Option<Node> {
        if hash == EMPTY_HASH {
            return None;
        }

        self.storage.get(&hash).map(Node::from_bytes)
    }

    fn as_mermaid_graph(&self) -> String {
        let mut graph = String::new();

        graph.push_str("graph TD;\n");

        if let Some(root) = self.get_node(self.root) {
            self.build_graph_string(&root, &mut graph);
        }

        graph
    }

    fn build_graph_string(&self, node: &Node, graph: &mut String) {
        dbg!(("building for", &node.key, &node.left, &node.right));

        let key = bytes_to_string(&node.key);
        let node_label = format!("{}({}:)", key, key);

        graph.push_str(&format!("    {};\n", node_label));

        if let Some(left) = self.get_node(node.left) {
            let key = bytes_to_string(&left.key);
            let left_label = format!("{}({})", key, key);

            graph.push_str(&format!("    {} --> {};\n", node_label, left_label));
            self.build_graph_string(&left, graph);
        }

        if let Some(right) = self.get_node(node.right) {
            let key = bytes_to_string(&right.key);
            let right_label = format!("{}({})", key, key);

            graph.push_str(&format!("    {} --> {};\n", node_label, right_label));
            self.build_graph_string(&right, graph);
        }
    }
}

impl Default for Treap {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}
fn bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b.to_string()).collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic() {
        let mut tree = Treap::default();

        for i in 0..4 {
            tree.insert(&[i], b"0");
        }

        dbg!(tree);
        // println!("{}", tree.as_mermaid_graph())
    }
}
