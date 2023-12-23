use redb::Table;
use std::cmp::Ordering;

use crate::node::{hash, Branch, Node};

#[derive(Debug)]
pub(crate) struct BinarySearchPath {
    pub upper: Vec<(Node, Branch)>,
    pub target: Option<Node>,
    pub lower: Vec<(Node, Branch)>,
}

/// Returns the binary search path for a given key in the following form:
/// - `upper` is the path with nodes with rank higher than the rank of the key.
/// - `target`      is the node with the exact same key (if any).
/// - `lower` is the path with nodes with rank lesss  than the rank of the key.
///
/// If a match was found, the `lower_path` will be empty.
pub(crate) fn binary_search_path(
    table: &Table<&'static [u8], (u64, &'static [u8])>,
    root: Option<Node>,
    key: &[u8],
) -> BinarySearchPath {
    let rank = hash(key);

    let mut path = BinarySearchPath {
        upper: Default::default(),
        target: None,
        lower: Default::default(),
    };

    let mut next = root;

    while let Some(current) = next {
        let stack = if current.rank().as_bytes() > rank.as_bytes() {
            &mut path.upper
        } else {
            &mut path.lower
        };

        match key.cmp(current.key()) {
            Ordering::Equal => {
                // We found exact match. terminate the search.

                path.target = Some(current);
                return path;
            }
            Ordering::Less => {
                next = current.left().and_then(|n| Node::open(table, n));

                stack.push((current, Branch::Left));
            }
            Ordering::Greater => {
                next = current.right().and_then(|n| Node::open(table, n));

                stack.push((current, Branch::Right));
            }
        };
    }

    path
}