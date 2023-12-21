use crate::node::Node;
use crate::treap::HashTreap;
use crate::Hash;

use redb::backends::InMemoryBackend;
use redb::Database;

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

    let upsert_at_root = ["X", "X"]
        .iter()
        .enumerate()
        .map(|(i, _)| {
            (
                Entry {
                    key: b"X".to_vec(),
                    value: i.to_string().into(),
                },
                Operation::Insert,
            )
        })
        .collect::<Vec<_>>();

    // X has higher rank.
    let upsert_deeper = ["X", "F", "F"]
        .iter()
        .enumerate()
        .map(|(i, key)| {
            (
                Entry {
                    key: key.as_bytes().to_vec(),
                    value: i.to_string().into(),
                },
                Operation::Insert,
            )
        })
        .collect::<Vec<_>>();

    let mut upsert_deeper_expected = upsert_deeper.clone();
    upsert_deeper_expected.remove(upsert_deeper.len() - 2);

    // X has higher rank.
    let upsert_root_with_children = ["F", "X", "X"]
        .iter()
        .enumerate()
        .map(|(i, key)| {
            (
                Entry {
                    key: key.as_bytes().to_vec(),
                    value: i.to_string().into(),
                },
                Operation::Insert,
            )
        })
        .collect::<Vec<_>>();

    let mut upsert_root_with_children_expected = upsert_root_with_children.clone();
    upsert_root_with_children_expected.remove(upsert_root_with_children.len() - 2);

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
            "upsert at root without children",
            upsert_at_root.clone(),
            upsert_at_root[1..]
                .iter()
                .map(|(e, _)| e.clone())
                .collect::<Vec<_>>(),
            Some("b1353174e730b9ff6850577357fd9ff608071bbab46ebe72c434133f5d4f0383"),
        ),
        (
            "upsert deeper",
            upsert_deeper.to_vec(),
            upsert_deeper_expected
                .to_vec()
                .iter()
                .map(|(e, _)| e.clone())
                .collect::<Vec<_>>(),
            Some("58272c9e8c9e6b7266e4b60e45d55257b94e85561997f1706e0891ee542a8cd5"),
        ),
        (
            "upsert at root with children",
            upsert_root_with_children.to_vec(),
            upsert_root_with_children_expected
                .to_vec()
                .iter()
                .map(|(e, _)| e.clone())
                .collect::<Vec<_>>(),
            Some("f46daf022dc852cd4e60a98a33de213f593e17bcd234d9abff7a178d8a5d0761"),
        ),
    ];

    for case in cases {
        test(case.0, &case.1, &case.2, case.3);
    }
}

// === Helpers ===

#[derive(Clone, Debug)]
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

fn test(name: &str, input: &[(Entry, Operation)], expected: &[Entry], root_hash: Option<&str>) {
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
        println!(
            "{:?} {:?}\n{}",
            &entry.key,
            &entry.value,
            into_mermaid_graph(&treap)
        );
    }

    let collected = treap
        .iter()
        .map(|n| Entry {
            key: n.key().to_vec(),
            value: n.value().to_vec(),
        })
        .collect::<Vec<_>>();

    let mut sorted = expected.to_vec();
    sorted.sort_by(|a, b| a.key.cmp(&b.key));

    // println!("{}", into_mermaid_graph(&treap));

    if root_hash.is_some() {
        assert_root(&treap, root_hash.unwrap());
    } else {
        dbg!(&treap.root_hash());

        verify_ranks(&treap);
    }

    assert_eq!(
        collected,
        sorted,
        "{}",
        format!("Entries do not match at: \"{}\"", name)
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
