use crate::node::Node;
use crate::treap::{HashTreap, NODES_TABLE};
use crate::Hash;

use redb::backends::InMemoryBackend;
use redb::{Database, Error, ReadableTable, TableDefinition};

#[test]
fn cases() {
    let sorted_alphabets = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ]
    .map(|key| Entry {
        key: key.as_bytes().to_vec(),
        value: [b"v", key.as_bytes()].concat(),
    });

    let mut reverse_alphabets = sorted_alphabets.clone();
    reverse_alphabets.reverse();

    let unsorted = ["D", "N", "P", "X", "A", "G", "C", "M", "H", "I", "J"].map(|key| Entry {
        key: key.as_bytes().to_vec(),
        value: [b"v", key.as_bytes()].concat(),
    });

    let single_entry = ["X"].map(|key| Entry {
        key: key.as_bytes().to_vec(),
        value: [b"v", key.as_bytes()].concat(),
    });

    let upsert_at_root = [
        (
            Entry {
                key: b"X".to_vec(),
                value: b"A".to_vec(),
            },
            Operation::Insert,
        ),
        ((
            Entry {
                key: b"X".to_vec(),
                value: b"B".to_vec(),
            },
            Operation::Insert,
        )),
    ];

    let upsert_deeper = [
        (
            Entry {
                key: b"F".to_vec(),
                value: b"A".to_vec(),
            },
            Operation::Insert,
        ),
        (
            Entry {
                key: b"X".to_vec(),
                value: b"A".to_vec(),
            },
            Operation::Insert,
        ),
        ((
            Entry {
                key: b"X".to_vec(),
                value: b"B".to_vec(),
            },
            Operation::Insert,
        )),
    ];

    let cases = [
        (
            "sorted alphabets",
            sorted_alphabets
                .clone()
                .map(|e| (e, Operation::Insert))
                .to_vec(),
            sorted_alphabets.to_vec(),
            Some("02af3de6ed6368c5abc16f231a17d1140e7bfec483c8d0aa63af4ef744d29bc3"),
        ),
        (
            "reversed alphabets",
            sorted_alphabets
                .clone()
                .map(|e| (e, Operation::Insert))
                .to_vec(),
            sorted_alphabets.to_vec(),
            Some("02af3de6ed6368c5abc16f231a17d1140e7bfec483c8d0aa63af4ef744d29bc3"),
        ),
        (
            "unsorted alphabets",
            unsorted.clone().map(|e| (e, Operation::Insert)).to_vec(),
            unsorted.to_vec(),
            Some("0957cc9b87c11cef6d88a95328cfd9043a3d6a99e9ba35ee5c9c47e53fb6d42b"),
        ),
        (
            "Single insert",
            single_entry
                .clone()
                .map(|e| (e, Operation::Insert))
                .to_vec(),
            single_entry.to_vec(),
            Some("b3e862d316e6f5caca72c8f91b7a15015b4f7f8f970c2731433aad793f7fe3e6"),
        ),
        (
            "upsert at root",
            upsert_at_root.to_vec(),
            upsert_at_root[1..]
                .iter()
                .map(|(e, _)| e.clone())
                .collect::<Vec<_>>(),
            Some("2947139081bbcc3816ebd73cb81ac0be5c564df55b88d6dbeb52c5254c1de887"),
        ),
        (
            "upsert deeper",
            upsert_deeper.to_vec(),
            upsert_at_root[0..2]
                .iter()
                .map(|(e, _)| e.clone())
                .collect::<Vec<_>>(),
            // Some("2947139081bbcc3816ebd73cb81ac0be5c564df55b88d6dbeb52c5254c1de887"),
            None,
        ),
    ];

    for case in cases {
        test(case.0, &case.1, &case.2, case.3);
    }
}

// === Helpers ===

#[derive(Clone)]
enum Operation {
    Insert,
    Delete,
}

#[derive(Clone, PartialEq)]
struct Entry {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?}, {:?})", self.key, self.value)
    }
}

fn test(name: &str, input: &[(Entry, Operation)], output: &[Entry], root_hash: Option<&str>) {
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

    let collected = treap
        .iter()
        .map(|n| Entry {
            key: n.key().to_vec(),
            value: n.value().to_vec(),
        })
        .collect::<Vec<_>>();

    let mut sorted = output.to_vec();
    sorted.sort_by(|a, b| a.key.cmp(&b.key));

    // dbg!(&treap.root_hash());
    println!("{}", into_mermaid_graph(&treap));

    if root_hash.is_some() {
        assert_root(&treap, root_hash.unwrap());
    }

    assert_eq!(
        collected,
        sorted,
        "{}",
        format!("Entries do not match at: \"{}\"", name)
    );
}

/// Verify ranks, and keys order
fn verify(treap: &HashTreap, entries: &[(&[u8], Vec<u8>)]) {
    verify_ranks(treap);
    verify_entries(
        treap,
        entries
            .iter()
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
            .collect::<Vec<_>>(),
    );
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

/// Verify that the expected entries are both sorted and present in the treap.
fn verify_entries(treap: &HashTreap, entries: Vec<(Vec<u8>, Vec<u8>)>) {
    let collected = treap
        .iter()
        .map(|n| (n.key().to_vec(), n.value().to_vec()))
        .collect::<Vec<_>>();

    let mut sorted = entries.iter().cloned().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(collected, sorted, "Entries do not match");
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
