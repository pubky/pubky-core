use std::assert_eq;
use std::collections::BTreeMap;

use crate::node::Node;
use crate::treap::HashTreap;
use crate::Hash;

use redb::backends::InMemoryBackend;
use redb::Database;

// === Helpers ===

#[derive(Clone, Debug)]
pub enum Operation {
    Insert,
    Delete,
}

#[derive(Clone, PartialEq)]
pub struct Entry {
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?}, {:?})", self.key, self.value)
    }
}

pub fn test_operations(input: &[(Entry, Operation)], root_hash: Option<&str>) {
    let inmemory = InMemoryBackend::new();
    let db = Database::builder()
        .create_with_backend(inmemory)
        .expect("Failed to create DB");

    let mut treap = HashTreap::new(&db, "test");

    for (entry, operation) in input {
        match operation {
            Operation::Insert => treap.insert(&entry.key, &entry.value),
            Operation::Delete => todo!(),
        }
    }

    // Uncomment to see the graph (only if values are utf8)
    // println!("{}", into_mermaid_graph(&treap));

    let collected = treap
        .iter()
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

    verify_ranks(&treap);

    let mut btree = BTreeMap::new();
    for (entry, operation) in input {
        match operation {
            Operation::Insert => {
                btree.insert(&entry.key, &entry.value);
            }
            Operation::Delete => {
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
        assert_root(&treap, root_hash.unwrap());
    }
}

/// Verify that every node has higher rank than its children.
fn verify_ranks(treap: &HashTreap) {
    assert!(
        verify_children_rank(treap, treap.root()),
        "Ranks are not sorted correctly"
    )
}

fn verify_children_rank(treap: &HashTreap, node: Option<Node>) -> bool {
    match node {
        Some(n) => {
            let left_check = treap.get_node(n.left()).map_or(true, |left| {
                n.rank().as_bytes() > left.rank().as_bytes()
                    && verify_children_rank(treap, Some(left))
            });
            let right_check = treap.get_node(n.right()).map_or(true, |right| {
                n.rank().as_bytes() > right.rank().as_bytes()
                    && verify_children_rank(treap, Some(right))
            });

            left_check && right_check
        }
        None => true,
    }
}

fn assert_root(treap: &HashTreap, expected_root_hash: &str) {
    let root_hash = treap
        .root()
        .map(|n| n.hash())
        .expect("Has root hash after insertion");

    assert_eq!(
        root_hash,
        Hash::from_hex(expected_root_hash).expect("Invalid hash hex"),
        "Root hash is not correct"
    )
}

// === Visualize the treap to verify the structure ===

fn into_mermaid_graph(treap: &HashTreap) -> String {
    let mut graph = String::new();

    graph.push_str("graph TD;\n");

    if let Some(root) = treap.root() {
        build_graph_string(&treap, &root, &mut graph);
    }

    graph.push_str(&format!(
        "    classDef null fill:#1111,stroke-width:1px,color:#fff,stroke-dasharray: 5 5;\n"
    ));

    graph
}

fn build_graph_string(treap: &HashTreap, node: &Node, graph: &mut String) {
    let key = bytes_to_string(node.key());
    let node_label = format!("{}(({}))", node.hash(), key);

    // graph.push_str(&format!("## START node {}\n", node_label));
    if let Some(child) = treap.get_node(node.left()) {
        let key = bytes_to_string(child.key());
        let child_label = format!("{}(({}))", child.hash(), key);

        graph.push_str(&format!("    {} --l--> {};\n", node_label, child_label));
        build_graph_string(&treap, &child, graph);
    } else {
        graph.push_str(&format!("    {} -.-> {}l((l));\n", node_label, node.hash()));
        graph.push_str(&format!("    class {}l null;\n", node.hash()));
    }

    if let Some(child) = treap.get_node(node.right()) {
        let key = bytes_to_string(child.key());
        let child_label = format!("{}(({}))", child.hash(), key);

        graph.push_str(&format!("    {} --r--> {};\n", node_label, child_label));
        build_graph_string(&treap, &child, graph);
    } else {
        graph.push_str(&format!("    {} -.-> {}r((r));\n", node_label, node.hash()));
        graph.push_str(&format!("    class {}r null;\n", node.hash()));
    }
}

fn bytes_to_string(byte: &[u8]) -> String {
    String::from_utf8(byte.to_vec()).expect("Invalid utf8 key in test with mermaig graph")
}
