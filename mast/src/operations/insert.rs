use blake3::Hash;
use redb::Table;

use super::{read::root_node_inner, search::binary_search_path, NODES_TABLE, ROOTS_TABLE};
use crate::node::{Branch, Node};

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
//     We call the rest of the path `unzipping path` or `lower path` and this is where we create two
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
//  After unzipping the lower path, we should get:
//
//         F
//          \
//           P
//          /
//         J
//        / \
//       H   M
//        \
//         I
//
//  So the end result beocmes:
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
//

pub(crate) fn insert(
    write_txn: &mut redb::WriteTransaction,
    treap: &str,
    key: &[u8],
    value: &[u8],
) -> Option<Node> {
    let mut roots_table = write_txn.open_table(ROOTS_TABLE).unwrap();
    let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

    let old_root = root_node_inner(&roots_table, &nodes_table, treap);

    let mut path = binary_search_path(&nodes_table, old_root, key);

    let mut left_subtree: Option<Hash> = None;
    let mut right_subtree: Option<Hash> = None;

    // Unzip the lower path to get left and right children of the inserted node.
    for (node, branch) in path.lower.iter_mut().rev() {
        // Decrement the old version.
        node.decrement_ref_count().save(&mut nodes_table);

        match branch {
            Branch::Right => {
                node.set_right_child(left_subtree);
                left_subtree = Some(node.hash());
            }
            Branch::Left => {
                node.set_left_child(right_subtree);
                right_subtree = Some(node.hash());
            }
        }

        node.increment_ref_count().save(&mut nodes_table);
    }

    let mut new_root;

    if let Some(mut found) = path.found {
        if found.value() == value {
            // There is really nothing to update. Skip traversing upwards.
            return Some(found);
        }

        // Decrement the old version.
        found.decrement_ref_count().save(&mut nodes_table);

        // Else, update the value and rehashe the node so that we can update the hashes upwards.
        found
            .set_value(value)
            .increment_ref_count()
            .save(&mut nodes_table);

        new_root = found
    } else {
        // Insert the new node.
        let mut node = Node::new(key, value);

        node.set_left_child(left_subtree)
            .set_right_child(right_subtree)
            .increment_ref_count()
            .save(&mut nodes_table);

        new_root = node
    };

    let mut upper_path = path.upper;

    // Propagate the new hashes upwards if there are any nodes in the upper_path.
    while let Some((mut node, branch)) = upper_path.pop() {
        node.decrement_ref_count().save(&mut nodes_table);

        match branch {
            Branch::Left => node.set_left_child(Some(new_root.hash())),
            Branch::Right => node.set_right_child(Some(new_root.hash())),
        };

        node.increment_ref_count().save(&mut nodes_table);

        new_root = node;
    }

    // Finally set the new root .
    roots_table
        .insert(treap.as_bytes(), new_root.hash().as_bytes().as_slice())
        .unwrap();

    // No older value was found.
    None
}

#[cfg(test)]
mod test {
    use crate::test::{test_operations, Entry};
    use proptest::prelude::*;

    proptest! {
        #[test]
        /// Test that upserting an entry with the same key in different tree shapes results in the
        /// expected structure
        fn test_upsert(random_entries in prop::collection::vec(
            (prop::collection::vec(any::<u8>(), 1), prop::collection::vec(any::<u8>(), 1)),
            1..10,
        )) {
            let operations = random_entries.into_iter().map(|(key, value)| {
                Entry::insert(&key, &value)
            }).collect::<Vec<_>>();

            test_operations(&operations, None);
        }

        #[test]
        fn test_general_insertiong(random_entries in prop::collection::vec(
            (prop::collection::vec(any::<u8>(), 32), prop::collection::vec(any::<u8>(), 32)),
            1..50,
        )) {
            let operations = random_entries.into_iter().map(|(key, value)| {
                Entry::insert(&key, &value)
            }).collect::<Vec<_>>();

            test_operations(&operations, None);
        }
    }

    #[test]
    fn insert_single_entry() {
        let case = ["A"];

        test_operations(
            &case.map(|key| Entry::insert(key.as_bytes(), &[b"v", key.as_bytes()].concat())),
            Some("9fbdb0a2023f8029871b44722b2091a45b8209eaa5ce912740959fc00c611b91"),
        )
    }

    #[test]
    fn sorted_alphabets() {
        let case = [
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ];

        test_operations(
            &case.map(|key| Entry::insert(key.as_bytes(), &[b"v", key.as_bytes()].concat())),
            Some("26820b21fec1451a2478808bb8bc3ade05dcfbcd50d9556cca77d12d6239f4a7"),
        );
    }

    #[test]
    fn reverse_alphabets() {
        let mut case = [
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ];
        case.reverse();

        test_operations(
            &case.map(|key| Entry::insert(key.as_bytes(), &[b"v", key.as_bytes()].concat())),
            Some("26820b21fec1451a2478808bb8bc3ade05dcfbcd50d9556cca77d12d6239f4a7"),
        )
    }

    #[test]
    fn unsorted() {
        let case = ["D", "N", "P", "X", "A", "G", "C", "M", "H", "I", "J"];

        test_operations(
            &case.map(|key| Entry::insert(key.as_bytes(), &[b"v", key.as_bytes()].concat())),
            Some("96c3cff677fb331fe2901a6b5297395f089a38af9ab4ad310d362f557d60fca5"),
        )
    }

    #[test]
    fn upsert_at_root() {
        let case = ["X", "X"];

        let mut i = 0;

        test_operations(
            &case.map(|key| {
                i += 1;
                Entry::insert(key.as_bytes(), i.to_string().as_bytes())
            }),
            Some("69e8b408d10174feb9d9befd0a3de95767cc0e342d0dba5f51139f4b49588fb7"),
        )
    }

    #[test]
    fn upsert_deeper() {
        // X has higher rank.
        let case = ["X", "F", "F"];

        let mut i = 0;

        test_operations(
            &case.map(|key| {
                i += 1;
                Entry::insert(key.as_bytes(), i.to_string().as_bytes())
            }),
            Some("9e73a80068adf0fb31382eb35d489aa9b50f91a3ad8e55523d5cca6d6247b15b"),
        )
    }

    #[test]
    fn upsert_root_with_children() {
        // X has higher rank.
        let case = ["F", "X", "X"];

        let mut i = 0;

        test_operations(
            &case.map(|key| {
                i += 1;
                Entry::insert(key.as_bytes(), i.to_string().as_bytes())
            }),
            Some("8c3cb6bb83df437b73183692e4b1b3809afd6974aec49d67b1ce3266e909cb67"),
        )
    }
}
