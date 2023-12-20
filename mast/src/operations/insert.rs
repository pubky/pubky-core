use std::cmp::Ordering;

use crate::node::{rank, Branch, Node};
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

pub fn insert(
    table: &'_ mut Table<&'static [u8], (u64, &'static [u8])>,
    root: Option<Hash>,
    key: &[u8],
    value: &[u8],
) -> Hash {
    let mut path = binary_search_path(table, root, key);

    let mut unzip_left_root: Option<Hash> = None;
    let mut unzip_right_root: Option<Hash> = None;

    for (node, branch) in path.unzip_path.iter_mut().rev() {
        match branch {
            Branch::Right => unzip_left_root = Some(node.set_right_child(table, unzip_left_root)),
            Branch::Left => unzip_right_root = Some(node.set_left_child(table, unzip_right_root)),
        }
    }

    let mut root = Node::insert(table, key, value, unzip_left_root, unzip_right_root);

    for (node, branch) in path.upper_path.iter_mut().rev() {
        match branch {
            Branch::Left => root = node.set_left_child(table, Some(root)),
            Branch::Right => root = node.set_right_child(table, Some(root)),
        }
    }

    // Finally return the new root to be committed.
    root
}

struct BinarySearchPath {
    upper_path: Vec<(Node, Branch)>,
    exact_match: Option<Node>,
    unzip_path: Vec<(Node, Branch)>,
}

fn binary_search_path(
    table: &'_ mut Table<&'static [u8], (u64, &'static [u8])>,
    root: Option<Hash>,
    key: &[u8],
) -> BinarySearchPath {
    let rank = rank(key);

    let mut result = BinarySearchPath {
        upper_path: Default::default(),
        exact_match: None,
        unzip_path: Default::default(),
    };

    let mut previous_hash = root;

    while let Some(current_hash) = previous_hash {
        let current_node = Node::open(table, current_hash).expect("Node not found!");

        // Decrement each node in the binary search path.
        // if it doesn't change, we will increment it again later.
        //
        // It is important then to terminate the loop if we found an exact match,
        // as lower nodes shouldn't change then.
        current_node.decrement_ref_count(table);

        let mut path = if current_node.rank().as_bytes() > rank.as_bytes() {
            &mut result.upper_path
        } else {
            &mut result.unzip_path
        };

        match key.cmp(current_node.key()) {
            Ordering::Equal => {
                // We found exact match. terminate the search.
                return result;
            }
            Ordering::Less => {
                previous_hash = *current_node.left();

                path.push((current_node, Branch::Left));
            }
            Ordering::Greater => {
                previous_hash = *current_node.right();

                path.push((current_node, Branch::Right));
            }
        };
    }

    result
}
