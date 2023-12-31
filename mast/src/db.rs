//! Kytz Database

use crate::node::Node;
use crate::operations::read::{get_node, root_hash, root_node};
use crate::operations::{insert, remove};
use crate::{Hash, Result};

pub struct Database {
    pub(crate) inner: redb::Database,
}

impl Database {
    /// Create a new in-memory database.
    pub fn in_memory() -> Self {
        let backend = redb::backends::InMemoryBackend::new();
        let inner = redb::Database::builder()
            .create_with_backend(backend)
            .unwrap();

        Self { inner }
    }

    pub fn begin_write(&self) -> Result<WriteTransaction> {
        let txn = self.inner.begin_write().unwrap();
        WriteTransaction::new(txn)
    }

    pub fn iter(&self, treap: &str) -> TreapIterator<'_> {
        // TODO: save tables instead of opening a new one on every next() call.
        TreapIterator::new(self, treap.to_string())
    }

    // === Private Methods ===

    pub(crate) fn get_node(&self, hash: &Option<Hash>) -> Option<Node> {
        get_node(&self, hash)
    }

    pub(crate) fn root_hash(&self, treap: &str) -> Option<Hash> {
        root_hash(&self, treap)
    }

    pub(crate) fn root(&self, treap: &str) -> Option<Node> {
        root_node(self, treap)
    }
}

pub struct TreapIterator<'db> {
    db: &'db Database,
    treap: String,
    stack: Vec<Node>,
}

impl<'db> TreapIterator<'db> {
    fn new(db: &'db Database, treap: String) -> Self {
        let mut iter = TreapIterator {
            db,
            treap: treap.clone(),
            stack: Vec::new(),
        };

        if let Some(root) = db.root(&treap) {
            iter.push_left(root)
        };

        iter
    }

    fn push_left(&mut self, mut node: Node) {
        while let Some(left) = self.db.get_node(node.left()) {
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
                if let Some(right) = self.db.get_node(node.right()) {
                    self.push_left(right)
                }

                Some(node.clone())
            }
            _ => None,
        }
    }
}

pub struct WriteTransaction<'db> {
    inner: redb::WriteTransaction<'db>,
}

impl<'db> WriteTransaction<'db> {
    pub(crate) fn new(inner: redb::WriteTransaction<'db>) -> Result<Self> {
        Ok(Self { inner })
    }

    pub fn insert(
        &mut self,
        treap: &str,
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) -> Option<Node> {
        // TODO: validate key and value length.
        // key and value mast be less than 2^32 bytes.

        insert(&mut self.inner, treap, key.as_ref(), value.as_ref())
    }

    pub fn remove(&mut self, treap: &str, key: impl AsRef<[u8]>) -> Option<Node> {
        remove(&mut self.inner, treap, key.as_ref())
    }

    pub fn commit(self) -> Result<()> {
        self.inner.commit().map_err(|e| e.into())
    }
}
