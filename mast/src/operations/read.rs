use super::{ReadableTable, NODES_TABLE, ROOTS_TABLE};
use crate::node::Node;
use crate::{Database, Hash, HASH_LEN};

pub fn root_hash_inner(
    table: &'_ impl ReadableTable<&'static [u8], &'static [u8]>,
    treap: &str,
) -> Option<Hash> {
    let existing = table.get(treap.as_bytes()).unwrap();
    existing.as_ref()?;

    let hash = existing.unwrap();

    let hash: [u8; HASH_LEN] = hash.value().try_into().expect("Invalid root hash");

    Some(Hash::from_bytes(hash))
}

pub fn root_node_inner(
    roots_table: &'_ impl ReadableTable<&'static [u8], &'static [u8]>,
    nodes_table: &'_ impl ReadableTable<&'static [u8], (u64, &'static [u8])>,
    treap: &str,
) -> Option<Node> {
    root_hash_inner(roots_table, treap).and_then(|hash| Node::open(nodes_table, hash))
}

pub fn get_node(db: &Database, hash: &Option<Hash>) -> Option<Node> {
    let read_txn = db.inner.begin_read().unwrap();
    let table = read_txn.open_table(NODES_TABLE).unwrap();

    hash.and_then(|h| Node::open(&table, h))
}

pub fn root_hash(db: &Database, treap: &str) -> Option<Hash> {
    let read_txn = db.inner.begin_read().unwrap();
    let table = read_txn.open_table(ROOTS_TABLE).unwrap();

    root_hash_inner(&table, treap)
}

pub fn root_node(db: &Database, treap: &str) -> Option<Node> {
    let read_txn = db.inner.begin_read().unwrap();
    let roots_table = read_txn.open_table(ROOTS_TABLE).unwrap();
    let nodes_table = read_txn.open_table(NODES_TABLE).unwrap();

    root_node_inner(&roots_table, &nodes_table, treap)
}
