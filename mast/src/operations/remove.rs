use blake3::Hash;
use redb::Table;

use super::{read::root_node_inner, search::binary_search_path, NODES_TABLE, ROOTS_TABLE};
use crate::node::{Branch, Node};

/// Removes the target node if it exists, and returns the new root and the removed node.
pub(crate) fn remove(
    write_txn: &mut redb::WriteTransaction,
    treap: &str,
    key: &[u8],
) -> Option<Node> {
    let mut roots_table = write_txn.open_table(ROOTS_TABLE).unwrap();
    let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

    let old_root = root_node_inner(&roots_table, &nodes_table, treap);

    let mut path = binary_search_path(&nodes_table, old_root, key);

    let mut new_root = None;

    if let Some(mut target) = path.found.clone() {
        new_root = zip(&mut nodes_table, &mut target)
    } else {
        // clearly the lower path has the highest node, and it won't be changed.
        new_root = path.lower.first().map(|(n, _)| n.clone());
    }

    // If there is an upper path, we propagate the hash updates upwards.
    while let Some((mut node, branch)) = path.upper.pop() {
        node.decrement_ref_count().save(&mut nodes_table);

        match branch {
            Branch::Left => node.set_left_child(new_root.map(|mut n| n.hash())),
            Branch::Right => node.set_right_child(new_root.map(|mut n| n.hash())),
        };

        node.increment_ref_count().save(&mut nodes_table);

        new_root = Some(node);
    }

    if let Some(mut new_root) = new_root {
        roots_table
            .insert(treap.as_bytes(), new_root.hash().as_bytes().as_slice())
            .unwrap();
    } else {
        roots_table.remove(treap.as_bytes()).unwrap();
    }

    path.found
}

fn zip(
    nodes_table: &'_ mut Table<&'static [u8], (u64, &'static [u8])>,
    target: &mut Node,
) -> Option<Node> {
    target.decrement_ref_count();
    target.save(nodes_table);

    let mut left_subtree = Vec::new();
    let mut right_subtree = Vec::new();

    if let Some(n) = target.left().and_then(|h| Node::open(nodes_table, h)) {
        left_subtree.push(n)
    }

    if let Some(n) = target.right().and_then(|h| Node::open(nodes_table, h)) {
        right_subtree.push(n)
    }

    while let Some(next) = left_subtree
        .last()
        .and_then(|n| n.right().and_then(|h| Node::open(nodes_table, h)))
    {
        left_subtree.push(next);
    }

    while let Some(next) = right_subtree
        .last()
        .and_then(|n| n.left().and_then(|h| Node::open(nodes_table, h)))
    {
        right_subtree.push(next);
    }

    let mut i = left_subtree.len().max(right_subtree.len());
    let mut previous: Option<Node> = None;

    while i > 0 {
        previous = zip_up(
            nodes_table,
            previous,
            left_subtree.get_mut(i - 1),
            right_subtree.get_mut(i - 1),
        );

        i -= 1;
    }

    previous
}

fn zip_up(
    nodes_table: &'_ mut Table<&'static [u8], (u64, &'static [u8])>,
    previous: Option<Node>,
    left: Option<&mut Node>,
    right: Option<&mut Node>,
) -> Option<Node> {
    match (left, right) {
        (Some(left), None) => Some(left.clone()), // Left  subtree is deeper
        (None, Some(right)) => Some(right.clone()), // Right subtree is deeper
        (Some(left), Some(right)) => {
            let rank_left = left.rank();
            let rank_right = right.rank();

            if left.rank().as_bytes() > right.rank().as_bytes() {
                right
                    // decrement old version
                    .decrement_ref_count()
                    .save(nodes_table)
                    // save new version
                    .set_left_child(previous.map(|mut n| n.hash()))
                    .increment_ref_count()
                    .save(nodes_table);

                left
                    // decrement old version
                    .decrement_ref_count()
                    .save(nodes_table)
                    // save new version
                    .set_right_child(Some(right.hash()))
                    .increment_ref_count()
                    .save(nodes_table);

                Some(left.clone())
            } else {
                left
                    // decrement old version
                    .decrement_ref_count()
                    .save(nodes_table)
                    // save new version
                    .set_right_child(previous.map(|mut n| n.hash()))
                    .increment_ref_count()
                    .save(nodes_table);

                right
                    // decrement old version
                    .decrement_ref_count()
                    .save(nodes_table)
                    // save new version
                    .set_left_child(Some(left.hash()))
                    .increment_ref_count()
                    .save(nodes_table);

                Some(right.clone())
            }
        }
        _ => {
            // Should never happen!
            None
        }
    }
}

#[cfg(test)]
mod test {
    use crate::test::{test_operations, Entry, Operation};
    use proptest::prelude::*;

    fn operation_strategy() -> impl Strategy<Value = Operation> {
        prop_oneof![
            // For cases without data, `Just` is all you need
            Just(Operation::Insert),
            Just(Operation::Remove),
        ]
    }

    proptest! {
        #[test]
        fn insert_remove(
            random_entries in prop::collection::vec(
                (prop::collection::vec(any::<u8>(), 1), prop::collection::vec(any::<u8>(), 1), operation_strategy()),
                1..10,
        )) {
            let operations = random_entries
                .into_iter()
                .map(|(key, value, op)| (Entry::new(&key, &value), op))
                .collect::<Vec<_>>();

            test_operations(&operations, None);
        }
    }

    #[test]
    fn becomes_empty() {
        let case = [("A", Operation::Insert), ("A", Operation::Remove)]
            .map(|(k, op)| (Entry::new(k.as_bytes(), k.as_bytes()), op));

        test_operations(&case, None)
    }

    #[test]
    fn lower_path() {
        let case = [Entry::insert(&[120], &[0]), Entry::remove(&[28])];

        test_operations(&case, None)
    }

    #[test]
    fn remove_with_lower() {
        let case = [
            Entry::insert(&[23], &[0]),
            Entry::insert(&[0], &[0]),
            Entry::remove(&[23]),
        ];

        test_operations(&case, None)
    }

    #[test]
    fn remove_with_upper() {
        let case = [Entry::insert(&[88], &[0]), Entry::remove(&[0])];

        test_operations(&case, None)
    }

    #[test]
    fn alphabet_after_remove() {
        let mut case = [
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ]
        .map(|key| Entry::insert(key.as_bytes(), &[b"v", key.as_bytes()].concat()))
        .to_vec();

        case.push(Entry::insert(&[0], &[0]));
        case.push(Entry::remove(&[0]));

        test_operations(
            &case,
            Some("26820b21fec1451a2478808bb8bc3ade05dcfbcd50d9556cca77d12d6239f4a7"),
        );
    }
}
