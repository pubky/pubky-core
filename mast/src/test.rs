//! Test helpers for the merkle treap.

use std::assert_eq;
use std::collections::BTreeMap;

use crate::node::Node;
use crate::Database;
use crate::Hash;

#[derive(Clone, Debug)]
pub enum Operation {
    Insert,
    Remove,
}

#[derive(Clone, PartialEq)]
pub struct Entry {
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
}

impl Entry {
    pub fn new(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.to_vec(),
            value: value.to_vec(),
        }
    }
    pub fn insert(key: &[u8], value: &[u8]) -> (Self, Operation) {
        (
            Self {
                key: key.to_vec(),
                value: value.to_vec(),
            },
            Operation::Insert,
        )
    }
    pub fn remove(key: &[u8]) -> (Self, Operation) {
        (
            Self {
                key: key.to_vec(),
                value: b"".to_vec(),
            },
            Operation::Remove,
        )
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?}, {:?})", self.key, self.value)
    }
}

pub fn test_operations(input: &[(Entry, Operation)], root_hash: Option<&str>) {
    let db = Database::in_memory();
    let mut txn = db.begin_write().unwrap();
    let treap = "test";

    for (entry, operation) in input {
        match operation {
            Operation::Insert => txn.insert(treap, &entry.key, &entry.value),
            Operation::Remove => txn.remove(treap, &entry.key),
        };
    }

    txn.commit();

    // Uncomment to see the graph
    // println!("{}", into_mermaid_graph(&treap));

    let collected = db
        .iter(treap)
        .map(|n| {
            assert_eq!(
                *n.ref_count(),
                1_u64,
                "{}",
                format!("Node has wrong ref count {:?}", n)
            );

            Entry {
                key: n.key().to_vec(),
                value: n.value().to_vec(),
            }
        })
        .collect::<Vec<_>>();

    verify_ranks(&db, treap);

    let mut btree = BTreeMap::new();
    for (entry, operation) in input {
        match operation {
            Operation::Insert => {
                btree.insert(&entry.key, &entry.value);
            }
            Operation::Remove => {
                btree.remove(&entry.key);
            }
        }
    }

    let expected = btree
        .iter()
        .map(|(key, value)| Entry {
            key: key.to_vec(),
            value: value.to_vec(),
        })
        .collect::<Vec<_>>();

    assert_eq!(collected, expected, "{}", format!("Entries do not match"));

    if root_hash.is_some() {
        assert_root(&db, treap, root_hash.unwrap());
    }
}

/// Verify that every node has higher rank than its children.
fn verify_ranks(db: &Database, treap: &str) {
    assert!(
        verify_children_rank(db, treap, db.root(treap)),
        "Ranks are not sorted correctly"
    )
}

fn verify_children_rank(db: &Database, treap: &str, node: Option<Node>) -> bool {
    match node {
        Some(n) => {
            let left_check = db.get_node(n.left()).map_or(true, |left| {
                n.rank().as_bytes() > left.rank().as_bytes()
                    && verify_children_rank(db, treap, Some(left))
            });
            let right_check = db.get_node(n.right()).map_or(true, |right| {
                n.rank().as_bytes() > right.rank().as_bytes()
                    && verify_children_rank(db, treap, Some(right))
            });

            left_check && right_check
        }
        None => true,
    }
}

fn assert_root(db: &Database, treap: &str, expected_root_hash: &str) {
    let root_hash = db.root_hash(treap).expect("Has root hash after insertion");

    assert_eq!(
        root_hash,
        Hash::from_hex(expected_root_hash).expect("Invalid hash hex"),
        "Root hash is not correct"
    )
}

// === Visualize the treap to verify the structure ===

fn into_mermaid_graph(db: &Database, treap: &str) -> String {
    let mut graph = String::new();

    graph.push_str("graph TD;\n");

    if let Some(mut root) = db.root(treap) {
        build_graph_string(db, treap, &mut root, &mut graph);
    }

    graph.push_str(&format!(
        "    classDef null fill:#1111,stroke-width:1px,color:#fff,stroke-dasharray: 5 5;\n"
    ));

    graph
}

fn build_graph_string(db: &Database, treap: &str, node: &mut Node, graph: &mut String) {
    let key = format_key(node.key());
    let node_label = format!("{}(({}))", node.hash(), key);

    // graph.push_str(&format!("## START node {}\n", node_label));
    if let Some(mut child) = db.get_node(node.left()) {
        let key = format_key(child.key());
        let child_label = format!("{}(({}))", child.hash(), key);

        graph.push_str(&format!("    {} --l--> {};\n", node_label, child_label));
        build_graph_string(db, treap, &mut child, graph);
    } else {
        graph.push_str(&format!("    {} -.-> {}l((l));\n", node_label, node.hash()));
        graph.push_str(&format!("    class {}l null;\n", node.hash()));
    }

    if let Some(mut child) = db.get_node(node.right()) {
        let key = format_key(child.key());
        let child_label = format!("{}(({}))", child.hash(), key);

        graph.push_str(&format!("    {} --r--> {};\n", node_label, child_label));
        build_graph_string(db, treap, &mut child, graph);
    } else {
        graph.push_str(&format!("    {} -.-> {}r((r));\n", node_label, node.hash()));
        graph.push_str(&format!("    class {}r null;\n", node.hash()));
    }
}

fn format_key(bytes: &[u8]) -> String {
    format!("\"{:?}\"", bytes)
}
