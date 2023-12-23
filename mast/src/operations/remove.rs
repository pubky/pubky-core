use blake3::Hash;
use redb::Table;

use super::search::binary_search_path;
use crate::node::{hash, Branch, Node};

pub(crate) fn remove<'a>(
    nodes_table: &'_ mut Table<&'static [u8], (u64, &'static [u8])>,
    root: Option<Node>,
    key: &[u8],
) -> Option<Node> {
    let mut path = binary_search_path(nodes_table, root, key);

    // The key doesn't exist, so there is nothing to remove.
    let mut root = path.upper.first().map(|(n, _)| n.clone());

    dbg!(&path);
    if let Some(mut target) = path.target {
        // Zipping

        target.decrement_ref_count();
        target.save(nodes_table);

        let mut left_subtree = Vec::new();
        let mut right_subtree = Vec::new();

        target
            .left()
            .and_then(|h| Node::open(nodes_table, h))
            .map(|n| left_subtree.push(n));

        while let Some(next) = left_subtree
            .last()
            .and_then(|n| n.right().and_then(|h| Node::open(nodes_table, h)))
        {
            left_subtree.push(next);
        }

        target
            .right()
            .and_then(|h| Node::open(nodes_table, h))
            .map(|n| right_subtree.push(n));

        while let Some(next) = right_subtree
            .last()
            .and_then(|n| n.left().and_then(|h| Node::open(nodes_table, h)))
        {
            right_subtree.push(next);
        }

        let mut i = left_subtree.len().max(right_subtree.len());
        let mut last: Option<Node> = None;

        while i > 0 {
            last = match (left_subtree.get_mut(i - 1), right_subtree.get_mut(i - 1)) {
                (Some(left), None) => Some(left.clone()), // Left  subtree is deeper
                (None, Some(right)) => Some(right.clone()), // Right subtree is deeper
                (Some(left), Some(right)) => {
                    let rank_left = hash(left.key());
                    let rank_right = hash(right.key());

                    if hash(left.key()).as_bytes() > hash(right.key()).as_bytes() {
                        right
                            // decrement old version
                            .decrement_ref_count()
                            .save(nodes_table)
                            // save new version
                            .set_left_child(last.map(|n| n.hash()))
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
                            .set_right_child(last.map(|n| n.hash()))
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
            };

            i -= 1;
        }

        // dbg!(&last);
        return last;
    } else {
        // clearly the lower path has the highest node, and it won't be changed.
        return path.lower.first().map(|(n, _)| n.clone());
    }

    if root.is_none() {
        root = path.lower.first().map(|(n, _)| n.clone());
    }

    return root;
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
        // #[test]
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

    // #[test]
    fn empty() {
        let case = [("A", Operation::Insert), ("A", Operation::Remove)]
            .map(|(k, op)| (Entry::new(k.as_bytes(), k.as_bytes()), op));

        test_operations(
            &case,
            Some("78fd7507ef338f1a5816ffd702394999680a9694a85f4b8af77795d9fdd5854d"),
        )
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
}
