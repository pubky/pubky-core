use blake3::{Hash, Hasher};

use crate::node::Branch;
use crate::storage::memory::MemoryStorage;
use crate::Node;

#[derive(Debug)]
pub struct HashTreap<'a> {
    pub(crate) storage: &'a mut MemoryStorage,
    pub(crate) root: Option<Hash>,
}

impl<'a> HashTreap<'a> {
    // TODO: add name to open from storage with.
    pub fn new(storage: &'a mut MemoryStorage) -> Self {
        Self {
            root: None,
            storage,
        }
    }

    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        // TODO: validate key and value length.

        let value = self.insert_blob(value);
        let mut node = Node::new(key, value);

        println!(
            "\n New insert {:?}",
            String::from_utf8(key.to_vec()).unwrap()
        );

        if self.root.is_none() {
            self.update_root(*node.hash());
            return;
        }

        // Watch this [video](https://youtu.be/NxRXhBur6Xs?si=GNwaUOfuGwr_tBKI&t=1763) for a good explanation of the unzipping algorithm.
        // Also see the Iterative insertion algorithm in the page 12 of the [original paper](https://arxiv.org/pdf/1806.06726.pdf).
        // The difference here is that in a Hash Treap, we need to update nodes bottom up.

        // Let's say we have the following tree:
        //
        //         F
        //        / \
        //       D   P
        //      /   / \
        //     C   H   X
        //    /   / \   \
        //   A   G   M   Y
        //          /
        //         I
        //
        // First we mark the binary search path to the leaf, going right if the key is greater than
        // the current node's key and vice versa.
        //
        //         F
        //          \
        //           P
        //          /
        //         H
        //          \
        //           M
        //          /
        //         I
        //

        // Path before insertion point. (Node, Branch to update)
        let mut top_path: Vec<(Node, Branch)> = Vec::new();
        // Subtree of nodes on the path smaller than the inserted key.
        let mut left_unzip_path: Vec<Node> = Vec::new();
        // Subtree of nodes on the path larger  than the inserted key.
        let mut right_unzip_path: Vec<Node> = Vec::new();

        let mut next = self.root;

        // Top down traversal of the binary search path.
        while let Some(current) = self.get_node(&next) {
            let should_zip = node.rank().as_bytes() > current.rank().as_bytes();

            // Traverse left or right.
            if key < current.key() {
                next = *current.left();

                if should_zip {
                    left_unzip_path.push(current)
                } else {
                    top_path.push((current, Branch::Left));
                }
            } else {
                next = *current.right();

                if should_zip {
                    right_unzip_path.push(current)
                } else {
                    top_path.push((current, Branch::Right));
                }
            };
        }
        dbg!((
            "Out of the first loop",
            &top_path,
            &left_unzip_path,
            &right_unzip_path
        ));

        // === Updating hashes bottom up ===

        // We are at the unzipping part of the path.
        //
        // First do the unzipping bottom up.
        //
        //         H
        //          \
        //           M    < current_right
        //          /
        //         I      < current_left
        //
        // Into (hopefully you can see the "unzipping"):
        //
        //  left     right
        //  subtree  subtree
        //
        //    H    |
        //      \  |
        //       I |  M

        while left_unzip_path.len() > 1 {
            let child = left_unzip_path.pop().unwrap();
            let mut parent = left_unzip_path.last_mut().unwrap();

            parent.set_child(&Branch::Right, Some(*child.hash()));
            parent.update(self.storage);
        }

        while right_unzip_path.len() > 1 {
            let child = right_unzip_path.pop().unwrap();
            let mut parent = right_unzip_path.last_mut().unwrap();

            parent.set_child(&Branch::Left, Some(*child.hash()));
            parent.update(self.storage);
        }

        // Done unzipping, join the current_left and current_right to J and update hashes upwards.
        //
        //         J     < Insertion point.
        //        / \
        //       H   M
        //        \
        //         I

        node.set_child(&Branch::Left, left_unzip_path.first().map(|n| *n.hash()));
        node.set_child(&Branch::Right, left_unzip_path.first().map(|n| *n.hash()));
        node.update(self.storage);

        // Update the rest of the path upwards with the new hashes.
        // So the final tree should look like:
        //
        //         F
        //        / \
        //       D   P
        //      /   / \
        //     C   J   X
        //    /   / \   \
        //   A   H   M   Y
        //      / \
        //     G   I

        if top_path.is_empty() {
            // The insertion point is at the root and we are done.
            self.update_root(*node.hash())
        }

        let mut previous = node;

        while let Some((mut parent, branch)) = top_path.pop() {
            parent.set_child(&branch, Some(*previous.hash()));
            parent.update(self.storage);

            previous = parent;
        }

        // Update the root pointer.
        self.update_root(*previous.hash())

        // Finally we should commit the changes to the storage.
        // TODO: commit
    }

    // === Private Methods ===

    fn update_root(&mut self, hash: Hash) {
        // The tree is empty, the incoming node has to be the root, and we are done.
        self.root = Some(hash);

        // TODO: we need to persist the root change too to the storage.
    }

    // TODO: Add stream input API.
    fn insert_blob(&mut self, blob: &[u8]) -> Hash {
        let mut hasher = Hasher::new();
        hasher.update(blob);
        let hash = hasher.finalize();

        self.storage.insert_blob(hash, blob);

        hash
    }

    pub(crate) fn get_node(&self, hash: &Option<Hash>) -> Option<Node> {
        hash.and_then(|h| self.storage.get_node(&h))
    }

    // === Test Methods ===

    #[cfg(test)]
    fn verify_ranks(&self) -> bool {
        let node = self.get_node(&self.root);
        self.check_rank(node)
    }

    #[cfg(test)]
    fn check_rank(&self, node: Option<Node>) -> bool {
        match node {
            Some(n) => {
                let left_check = self.get_node(n.left()).map_or(true, |left| {
                    n.rank().as_bytes() > left.rank().as_bytes() && self.check_rank(Some(left))
                });
                let right_check = self.get_node(n.right()).map_or(true, |right| {
                    n.rank().as_bytes() > right.rank().as_bytes() && self.check_rank(Some(right))
                });

                left_check && right_check
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod test {
    use super::HashTreap;
    use super::MemoryStorage;
    use super::Node;

    #[test]
    fn basic() {
        let mut storage = MemoryStorage::new();
        let mut treap = HashTreap::new(&mut storage);

        // let mut keys = ["A", "C", "D", "F", "G", "H", "M", "P", "X", "Y"];
        let mut keys = [
            "D", "N", "P", "X", "F", "Z", "Y", "A", "G", "C", "M", "H", "I", "J",
        ];
        // let mut keys = ["A", "B", "C"];
        // keys.reverse();
        // keys.reverse(); // Overflowing stack! damn recursion.

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks());
        // dbg!(&tree);
        println!("{}", treap.as_mermaid_graph())
    }
}
