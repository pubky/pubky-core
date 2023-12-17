use blake3::{Hash, Hasher};

use crate::node::Child;
use crate::storage::memory::MemoryStorage;
use crate::Node;

#[derive(Debug)]
pub struct Treap<'a> {
    pub(crate) storage: &'a mut MemoryStorage,
    pub(crate) root: Option<Node>,
}

// TODO: pass a transaction.
fn insert(
    node: &mut Node,
    root: Option<Hash>,
    storage: MemoryStorage,
    changed: &mut Vec<Node>,
) -> Node {
    let root = root.and_then(|hash| storage.get_node(&hash));

    if root.is_none() {
        return node.clone();
    }

    let mut root = root.unwrap();

    if node.key() < root.key() {
        if insert(node, *root.left(), storage, changed).key() == node.key() {
            if node.rank().as_bytes() < root.rank().as_bytes() {
                root.set_child_hash(Child::Left, *node.hash())
            } else {
                // root.set_child_hash(Child::Left, *node.right());
                node.set_child_hash(Child::Right, *root.hash());
            }
        }
    }

    return root;
}

impl<'a> Treap<'a> {
    // TODO: add name to open from storage with.
    pub fn new(storage: &'a mut MemoryStorage) -> Self {
        Self {
            root: None,
            storage,
        }
    }

    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        let value = self.insert_blob(value);
        let mut node = Node::new(key, value);

        let mut changed: Vec<Node> = vec![];

        insert(
            &mut node,
            Some(self.root.hash()),
            self.storage,
            &mut changed,
        )
    }

    // pub fn insert(&mut self, key: &[u8], value: &[u8]) {
    // let value = self.insert_blob(value);
    // let mut node = Node::new(key, value);
    //
    // // Watch this [video](https://youtu.be/NxRXhBur6Xs?si=GNwaUOfuGwr_tBKI&t=1763) for a good explanation of the unzipping algorithm.
    // // Also see the Iterative insertion algorithm in the page 12 of the [original paper](https://arxiv.org/pdf/1806.06726.pdf).
    //
    // // Let's say we have the following treap:
    // //
    // //         F
    // //        / \
    // //       D   P
    // //      /   / \
    // //     C   H   X
    // //    /   / \   \
    // //   A   G   M   Y
    // //          /
    // //         I
    // //
    // // We focus on the binary search path for J, in this case [F, P, H, M, I]:
    // //
    // //         F < J
    // //          \
    // //       J < P
    // //          /
    // //         H < J
    // //          \
    // //       J < M
    // //          /
    // //         I < J
    // //
    // // First we traverse until we reach the insertion point, in this case H,
    // // because J has a higher rank than H, but lower than F and P;
    //
    // let mut path: Vec<Node> = Vec::new();
    //
    // let mut current = self.root.clone();
    //
    // while let Some(curr) = current {
    //     if node.rank().as_bytes() > curr.rank().as_bytes() {
    //         // We reached the insertion point.
    //         // rank can't be equal, as we are using a secure hashing funciton.
    //         break;
    //     }
    //
    //     path.push(curr.clone());
    //
    //     if node.key() < curr.key() {
    //         current = self.get_node(curr.left());
    //     } else {
    //         current = self.get_node(curr.right());
    //     }
    // }
    //
    // if let Some(mut prev) = path.last_mut() {
    //     let old = prev.clone();
    //
    //     // TODO: pass transaction here.
    //     if node.key() < prev.key() {
    //         prev.set_child_hash(Child::Left, node.update_hash())
    //     } else {
    //         prev.set_child_hash(Child::Right, node.update_hash())
    //     }
    //
    //     self.storage.insert_node(&prev);
    //     dbg!((old, prev));
    // } else {
    //     // The insertion point is at the root node, either because the tree is empty,
    //     // or because the root node has lower rank than the new node.
    //
    //     self.root = Some(node.clone());
    // }
    //
    // dbg!(&path);
    //
    // // then Unzip the rest of the path:
    // //
    // // In the example above these are [H, M]
    // //
    // //         F
    // //          \
    // //           P
    // //          /
    // //         J < Insertion point.
    // //       /     connect J to H to the left
    // //      H < Unzip
    // //      \\
    // //       M
    // //      //
    // //     I
    // //
    // // if let Some(curr) = current {
    // //     if node.key() < curr.key() {
    // //         node.set_child_hash(Child::Right, *curr.hash())
    // //     } else {
    // //         node.set_child_hash(Child::Left, *curr.hash())
    // //     }
    // // } else {
    // //     // We reached the endo of the searhc path, and inserted a leaf node.
    // //     return;
    // // }
    //
    // // The unsizipped path should look like:
    // //
    // //         F
    // //          \
    // //           P
    // //          /
    // //         J
    // //       // \\
    // //       H   M  < See how that looks like unzipping? :)
    // //       \\
    // //        I
    // //
    //
    // // if let Some(curr) = current {
    // //     // We reached the insertion (unzipping point);
    // // } else {
    // //     // We reached the end of the search path, this is equivilant of
    // //     // J having lower rank than I, so we insert J as a leaf node.
    // //
    // //     // There has to be a node, because we already checked at the beginning
    // //     // that the tree is not empty.
    // //     if let Some(current_leaf) = previous {
    // //         if key < current_leaf.key() {
    // //             // Insert as a left child.
    // //             // let old_child = self.update_child(current_leaf, Child::Left, node);
    // //         } else {
    // //             // Insert as a right child.
    // //             let old_child = self.update_child(current_leaf, Child::Right, node);
    // //         }
    // //     }
    // // }
    //
    // // So the final tree should look like:
    // //
    // //         F
    // //        / \
    // //       D   P
    // //      /   / \
    // //     C   J   X
    // //    /   / \   \
    // //   A   H   M   Y
    // //      / \
    // //     G   I
    //
    // // Finally we should commit the changes to the storage.
    // // TODO: commit
    // }

    // TODO: Add stream input API.
    fn insert_blob(&mut self, blob: &[u8]) -> Hash {
        let mut hasher = Hasher::new();
        hasher.update(blob);
        let hash = hasher.finalize();

        self.storage.insert_blob(hash, blob);

        hash
    }

    // === Private Methods ===

    pub(crate) fn get_node(&self, hash: &Option<Hash>) -> Option<Node> {
        hash.and_then(|h| self.storage.get_node(&h))
    }

    // /// Replace a child of a node, and return the old child.
    // ///
    // /// Also decrements the ref_count of the old child,
    // /// and  incrments  the ref_count of the new child,
    // ///
    // /// but it dosn't flush any changes to the storage yet.
    // pub(crate) fn update_child(
    //     &self,
    //     node: &mut Node,
    //     child: Child,
    //     new_child: Node,
    // ) -> Option<Node> {
    //     // Decrement old child's ref count.
    //     let mut old_child = match child {
    //         Child::Left => node.left(),
    //         Child::Right => node.right(),
    //     }
    //     .and_then(|hash| self.storage.get_node(&hash));
    //     old_child.as_mut().map(|n| n.decrement_ref_count());
    //
    //     // Increment new child's ref count.
    //     node.increment_ref_count();
    //
    //     node.set_child_hash(child, node.hash().clone());
    //
    //     old_child
    // }
}
