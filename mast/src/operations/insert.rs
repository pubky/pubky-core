use blake3::Hash;
use redb::Table;

use super::search::binary_search_path;
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
    nodes_table: &'_ mut Table<&'static [u8], (u64, &'static [u8])>,
    root: Option<Node>,
    key: &[u8],
    value: &[u8],
) -> Node {
    let mut path = binary_search_path(nodes_table, root, key);

    let mut left_subtree: Option<Hash> = None;
    let mut right_subtree: Option<Hash> = None;

    // Unzip the lower path to get left and right children of the inserted node.
    for (node, branch) in path.lower.iter_mut().rev() {
        // Decrement the old version.
        node.decrement_ref_count().save(nodes_table);

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

        node.increment_ref_count().save(nodes_table);
    }

    let mut root;

    if let Some(mut target) = path.target {
        if target.value() == value {
            // There is really nothing to update. Skip traversing upwards.

            return path.upper.first().map(|(n, _)| n.clone()).unwrap_or(target);
        }

        // Decrement the old version.
        target.decrement_ref_count().save(nodes_table);

        // Else, update the value and rehashe the node so that we can update the hashes upwards.
        target
            .set_value(value)
            .increment_ref_count()
            .save(nodes_table);

        root = target
    } else {
        // Insert the new node.
        let mut node = Node::new(key, value);

        node.set_left_child(left_subtree)
            .set_right_child(right_subtree)
            .increment_ref_count()
            .save(nodes_table);

        root = node
    };

    let mut upper_path = path.upper;

    // Propagate the new hashes upwards if there are any nodes in the upper_path.
    while let Some((mut node, branch)) = upper_path.pop() {
        node.decrement_ref_count().save(nodes_table);

        match branch {
            Branch::Left => node.set_left_child(Some(root.hash())),
            Branch::Right => node.set_right_child(Some(root.hash())),
        };

        node.increment_ref_count().save(nodes_table);

        root = node;
    }

    // Finally return the new root to be set to the root.
    root
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
            Some("78fd7507ef338f1a5816ffd702394999680a9694a85f4b8af77795d9fdd5854d"),
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
            Some("02af3de6ed6368c5abc16f231a17d1140e7bfec483c8d0aa63af4ef744d29bc3"),
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
            Some("02af3de6ed6368c5abc16f231a17d1140e7bfec483c8d0aa63af4ef744d29bc3"),
        )
    }

    #[test]
    fn unsorted() {
        let case = ["D", "N", "P", "X", "A", "G", "C", "M", "H", "I", "J"];

        test_operations(
            &case.map(|key| Entry::insert(key.as_bytes(), &[b"v", key.as_bytes()].concat())),
            Some("0957cc9b87c11cef6d88a95328cfd9043a3d6a99e9ba35ee5c9c47e53fb6d42b"),
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
            Some("4538b4de5e58f9be9d54541e69fab8c94c31553a1dec579227ef9b572d1c1dff"),
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
            Some("c9f7aaefb18ec8569322b9621fc64f430a7389a790e0bf69ec0ad02879d6ce54"),
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
            Some("02e26311f2b55bf6d4a7163399f99e17c975891a05af2f1e09bc969f8bf0f95d"),
        )
    }
}
