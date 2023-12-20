use blake3::Hash;
use redb::*;

use crate::{node::Node, HASH_LEN};

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

    pub(crate) fn root(&self) -> Option<Node> {
        let read_txn = self.db.begin_read().unwrap();

        let roots_table = read_txn.open_table(ROOTS_TABLE).unwrap();
        let nodes_table = read_txn.open_table(NODES_TABLE).unwrap();

        self.root_hash(&roots_table)
            .and_then(|hash| Node::open(&nodes_table, hash))
    }

    fn root_hash<'a>(
        &self,
        table: &'a impl ReadableTable<&'static [u8], &'static [u8]>,
    ) -> Option<Hash> {
        let existing = table.get(self.name.as_bytes()).unwrap();
        if existing.is_none() {
            return None;
        }
        let hash = existing.unwrap();

        let hash: [u8; HASH_LEN] = hash.value().try_into().expect("Invalid root hash");

        Some(Hash::from_bytes(hash))
    }

    // === Public Methods ===

    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        // TODO: validate key and value length.

        let write_txn = self.db.begin_write().unwrap();

        'transaction: {
            let roots_table = write_txn.open_table(ROOTS_TABLE).unwrap();
            let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

            let root = self.root_hash(&roots_table);

            crate::operations::insert::insert(&mut nodes_table, root, key, value)
        };

        // Finally commit the changes to the storage.
        write_txn.commit().unwrap();
    }

    // === Private Methods ===

    /// Create a read transaction and get a node from the nodes table.
    pub(crate) fn get_node(&self, hash: &Option<Hash>) -> Option<Node> {
        let read_txn = self.db.begin_read().unwrap();
        let table = read_txn.open_table(NODES_TABLE).unwrap();

        hash.and_then(|h| Node::open(&table, h))
    }

    // === Test Methods ===

    // TODO: move tests and test helper methods to separate module.
    // Only keep the public methods here, and probably move it to lib.rs too.

    #[cfg(test)]
    fn verify_ranks(&self) -> bool {
        self.check_rank(self.root())
    }

    #[cfg(test)]
    fn check_rank(&self, node: Option<Node>) -> bool {
        match node {
            Some(n) => {
                let left_check = self.get_node(n.left()).map_or(true, |left| {
                    n.rank().as_bytes() > left.rank().as_bytes() && self.check_rank(Some(left))
                });
                let right_check = self.get_node(n.right()).map_or(true, |right| {
                    n.rank().as_bytes() > right.rank().as_bytes() && self.check_rank(Some(right))
                });

                left_check && right_check
            }
            None => true,
        }
    }

    #[cfg(test)]
    fn list_all_nodes(&self) {
        // TODO: return all the nodes to verify GC in the test, or verify it here.
        let read_txn = self.db.begin_read().unwrap();
        let nodes_table = read_txn.open_table(NODES_TABLE).unwrap();

        let mut iter = nodes_table.iter().unwrap();

        while let Some(existing) = iter.next() {
            let key;
            let data;
            let existing = existing.unwrap();
            {
                key = existing.0.value();
                data = existing.1.value();
            }

            // TODO: iterate over nodes
            // println!(
            //     "HEre is a node key:{:?} ref_count:{:?} node:{:?}",
            //     Hash::from_bytes(key.try_into().unwrap()),
            //     data.0,
            //     Node::open(data)
            // );
        }
    }
}

#[cfg(test)]
mod test {
    use super::HashTreap;
    use super::Node;

    use redb::backends::InMemoryBackend;
    use redb::{Database, Error, ReadableTable, TableDefinition};

    // TODO: write a good test for GC.

    #[test]
    fn sorted_insert() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Database::create(file.path()).unwrap();

        let mut treap = HashTreap::new(&db, "test");

        let mut keys = [
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ];

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks());
        println!("{}", treap.as_mermaid_graph())
    }

    #[test]
    fn unsorted_insert() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Database::create(file.path()).unwrap();

        let mut treap = HashTreap::new(&db, "test");

        // TODO: fix this cases
        let mut keys = [
            // "D", "N", "P",
            "X", // "F", "Z", "Y",
            "A", //
                 // "G", //
                 // "C", //
                 //"M", "H", "I", "J",
        ];

        // TODO: fix without sort.
        // keys.sort();

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks(), "Ranks are not correct");

        treap.list_all_nodes();

        println!("{}", treap.as_mermaid_graph())
    }

    #[test]
    fn upsert() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Database::create(file.path()).unwrap();

        let mut treap = HashTreap::new(&db, "test");

        let mut keys = ["X", "X"];

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks(), "Ranks are not correct");

        // TODO: check the value.

        println!("{}", treap.as_mermaid_graph())
    }

    #[test]
    fn upsert_deeper_than_root() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Database::create(file.path()).unwrap();

        let mut treap = HashTreap::new(&db, "test");

        let mut keys = ["F", "X", "X"];

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks(), "Ranks are not correct");

        // TODO: check the value.

        println!("{}", treap.as_mermaid_graph())
    }
}
