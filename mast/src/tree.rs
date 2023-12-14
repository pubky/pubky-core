//! Zip tree implementation.

use blake3::{Hash, Hasher};
use bytes::{Bytes, BytesMut};
use std::collections::btree_map::BTreeMap;
use std::fmt::{self, Debug, Display, Formatter};

use crate::node::Node;
use crate::storage;

#[derive(Debug)]
pub struct ZipTree {
    root: Option<Node>,
    storage: storage::memory::Storage,
}

impl ZipTree {
    pub fn new() -> Self {
        Self {
            root: None,
            storage: storage::memory::Storage::default(),
        }
    }

    pub fn insert(&mut self, key: &[u8], value: &[u8]) -> &mut Self {
        // let node = Node::new(key, value);
        //
        // let hash = node.hash();
        //
        // self.storage.insert(hash, node.serialize().into());
        // self.root = Some(hash);

        self
    }

    // pub fn node(&self, hash: &Hash) -> Option<Node> {
    // if let Some(encoded_node) = self.storage.get(hash) {
    //     return Node::deserialize(encoded_node).ok().or(None);
    // };
    //
    // None
    // }
}

// impl Display for ZipTree {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         writeln!(f, "graph TD;");
//
//         let mut stack: Vec<Node> = Vec::new();
//
//         if let Some(root_id) = self.root {
//             if let Some(node) = self.node(&root_id) {
//                 stack.push(node);
//             }
//         }
//
//         while let Some(node) = stack.pop() {
//             let left = self.node(&node.left());
//             let right = self.node(&node.right());
//
//             match (&left, &right) {
//                 (None, None) => {
//                     writeln!(f, "  {:?}", node.key());
//                 }
//                 _ => {
//                     if let Some(left) = left {
//                         writeln!(f, "  {:?} --> {:?}", node.key(), left.key());
//                     }
//                     if let Some(right) = right {
//                         writeln!(f, "  {:?} --> {:?}", node.key(), right.key());
//                     }
//                 }
//             }
//         }
//
//         Ok(())
//     }
// }

// impl Debug for ZipTree {
//     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self);
//
//         Ok(())
//     }
// }

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_new() {
        let mut tree = ZipTree::new();

        tree.insert(b"foo", b"bar");

        dbg!(tree);
    }
}
