use blake3::Hash;
use redb::*;

use crate::{node::Node, HASH_LEN};

// TODO: test that order is correct
// TODO: test that there are no extr anodes.

// Table: Nodes v0
// stores all the hash treap nodes from all the treaps in the storage.
//
// Key:   `[u8; 32]`    # Node hash
// Value: `(u64, [u8])` # (RefCount, EncodedNode)
pub const NODES_TABLE: TableDefinition<&[u8], (u64, &[u8])> =
    TableDefinition::new("kytz:hash_treap:nodes:v0");

// Table: Roots v0
// stores all the current roots for all treaps in the storage.
//
// Key:   `[u8; 32]`    # Treap name
// Value: `[u8; 32]` # Hash
pub const ROOTS_TABLE: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("kytz:hash_treap:roots:v0");

#[derive(Debug)]
pub struct HashTreap<'treap> {
    /// Redb database to store the nodes.
    pub(crate) db: &'treap Database,
    pub(crate) name: &'treap str,
}

impl<'treap> HashTreap<'treap> {
    // TODO: add name to open from storage with.
    pub fn new(db: &'treap Database, name: &'treap str) -> Self {
        // Setup tables
        let write_tx = db.begin_write().unwrap();
        {
            let _table = write_tx.open_table(NODES_TABLE).unwrap();
            let _table = write_tx.open_table(ROOTS_TABLE).unwrap();
        }
        write_tx.commit().unwrap();

        Self { name, db }
    }

    // === Getters ===

    /// Returns the root hash of the treap.
    pub fn root_hash(&self) -> Option<Hash> {
        let read_txn = self.db.begin_read().unwrap();
        let table = read_txn.open_table(ROOTS_TABLE).unwrap();

        self.root_hash_inner(&table)
    }

    // === Public Methods ===

    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        // TODO: validate key and value length.
        // key and value mast be less than 2^32 bytes.

        let write_txn = self.db.begin_write().unwrap();

        {
            let mut roots_table = write_txn.open_table(ROOTS_TABLE).unwrap();
            let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

            let old_root = self
                .root_hash_inner(&roots_table)
                .and_then(|hash| Node::open(&nodes_table, hash));

            let mut new_root = crate::operations::insert(&mut nodes_table, old_root, key, value);

            roots_table
                .insert(self.name.as_bytes(), new_root.hash().as_bytes().as_slice())
                .unwrap();
        };

        // Finally commit the changes to the storage.
        write_txn.commit().unwrap();
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<Box<[u8]>> {
        let write_txn = self.db.begin_write().unwrap();

        let mut removed_node;

        {
            let mut roots_table = write_txn.open_table(ROOTS_TABLE).unwrap();
            let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

            let old_root = self
                .root_hash_inner(&roots_table)
                .and_then(|hash| Node::open(&nodes_table, hash));

            let (new_root, old_node) = crate::operations::remove(&mut nodes_table, old_root, key);

            removed_node = old_node;

            if let Some(mut new_root) = new_root {
                roots_table
                    .insert(self.name.as_bytes(), new_root.hash().as_bytes().as_slice())
                    .unwrap();
            } else {
                roots_table.remove(self.name.as_bytes()).unwrap();
            }
        };

        // Finally commit the changes to the storage.
        write_txn.commit().unwrap();

        removed_node.map(|node| node.value().to_vec().into_boxed_slice())
    }

    pub fn iter(&self) -> TreapIterator<'_> {
        TreapIterator::new(self)
    }

    // === Private Methods ===

    pub(crate) fn root(&self) -> Option<Node> {
        let read_txn = self.db.begin_read().unwrap();

        let roots_table = read_txn.open_table(ROOTS_TABLE).unwrap();
        let nodes_table = read_txn.open_table(NODES_TABLE).unwrap();

        self.root_hash_inner(&roots_table)
            .and_then(|hash| Node::open(&nodes_table, hash))
    }

    fn root_hash_inner(
        &self,
        table: &'_ impl ReadableTable<&'static [u8], &'static [u8]>,
    ) -> Option<Hash> {
        let existing = table.get(self.name.as_bytes()).unwrap();
        existing.as_ref()?;

        let hash = existing.unwrap();

        let hash: [u8; HASH_LEN] = hash.value().try_into().expect("Invalid root hash");

        Some(Hash::from_bytes(hash))
    }

    pub(crate) fn get_node(&self, hash: &Option<Hash>) -> Option<Node> {
        let read_txn = self.db.begin_read().unwrap();
        let table = read_txn.open_table(NODES_TABLE).unwrap();

        hash.and_then(|h| Node::open(&table, h))
    }
}

pub struct TreapIterator<'treap> {
    treap: &'treap HashTreap<'treap>,
    stack: Vec<Node>,
}

impl<'a> TreapIterator<'a> {
    fn new(treap: &'a HashTreap<'a>) -> Self {
        let mut iter = TreapIterator {
            treap,
            stack: Vec::new(),
        };

        if let Some(root) = treap.root() {
            iter.push_left(root)
        };

        iter
    }

    fn push_left(&mut self, mut node: Node) {
        while let Some(left) = self.treap.get_node(node.left()) {
            self.stack.push(node);
            node = left;
        }
        self.stack.push(node);
    }
}

impl<'a> Iterator for TreapIterator<'a> {
    type Item = Node;

    fn next(&mut self) -> Option<Self::Item> {
        match self.stack.pop() {
            Some(node) => {
                if let Some(right) = self.treap.get_node(node.right()) {
                    self.push_left(right)
                }

                Some(node.clone())
            }
            _ => None,
        }
    }
}
