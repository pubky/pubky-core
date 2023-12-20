use crate::node::{get_node, get_root_hash, rank, Branch, Node};
use crate::treap::{HashTreap, NODES_TABLE, ROOTS_TABLE};
use crate::HASH_LEN;
use blake3::Hash;
use redb::{Database, ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};

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
// The binary search path for inserting `J` then is:
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
// Then we define `upper_path` as the path from the root to the insertion point
// marked by the first node with a `rank` that is either:
//
// - less than the `rank` of the inserted key:
//
//         F
//          \
//           P
//    ∧--   /  --∧ upper path if rank(J) > rank(H)
//    ∨--  H   --∨ unzip path
//          \
//           M       Note that this is an arbitrary example,
//          /        do not expect the actual ranks of these keys to be the same in implmentation.
//         I
//
//     Upper path doesn't change much beyond updating the hash of their child in the branch featured in
//     this binary search path.
//
//     We call the rest of the path `unzipping path` or `split path` and this is where we create two
//     new paths (left and right), each contain the nodes with keys smaller than or larger than the
//     inserted key respectively.
//
//     We update these unzipped paths from the _bottom up_ to generate the new hashes for their
//     parents.
//     Once we have the two paths, we use their tips as the new children of the newly inserted node.
//     Finally we update the hashes upwards until we reach the new root of the tree.
//
// - equal to the `rank` of the inserted key:
//
//         F
//          \
//           P
//          /
//         H     --^ upper path if
//                rank(H) = rank(H)
//
//                   This (exact key match) is the only way for the rank to match
//                   for secure hashes like blake3.
//
//      This is a different case since we don't really need to split (unzip) the lower path, we just
//      need to update the hash of the node (according to the new value) and update the hash of its
//      parents until we reach the root.
//
//  Also note that we need to update the `ref_count` of all the nodes, and delete the nodes with
//  `ref_count` of zero.
//
//  The simplest way to do so, is to decrement all the nodes in the search path, and then increment
//  all then new nodes (in both the upper and lower paths) before comitting the write transaction.

impl<'treap> HashTreap<'treap> {
    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        // TODO: validate key and value length.

        let write_txn = self.db.begin_write().unwrap();

        'transaction: {
            let roots_table = write_txn.open_table(ROOTS_TABLE).unwrap();
            let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

            let root = get_root_hash(&roots_table, &self.name);

            let mut path = upper_path(key, root, &nodes_table);

            path.iter_mut()
                .for_each(|node| node.decrement_ref_count(&mut nodes_table))

            // if path.
        };

        // Finally commit the changes to the storage.
        write_txn.commit().unwrap();
    }
}

/// Returns the current nodes from the root to the insertion point on the binary search path.
fn upper_path<'a>(
    key: &[u8],
    root: Option<Hash>,
    nodes_table: &'a impl ReadableTable<&'static [u8], (u64, &'static [u8])>,
) -> Vec<Node> {
    let rank = rank(key);

    let mut path: Vec<Node> = Vec::new();

    let mut previous_hash = root;

    while let Some(current_hash) = previous_hash {
        let current_node = get_node(nodes_table, current_hash.as_bytes()).expect("Node not found!");

        let current_key = current_node.key();

        if key == current_key {
            // We found an exact match, we don't need to unzip the rest.
            path.push(current_node);
            break;
        }

        if key < current_key {
            previous_hash = *current_node.left();
        } else {
            previous_hash = *current_node.right();
        }

        path.push(current_node);
    }

    path
}
