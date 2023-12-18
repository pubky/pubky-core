use crate::storage::memory::MemoryStorage;
use crate::{Hash, Hasher, EMPTY_HASH};

// TODO: make sure that the hash is always in sync.
// TODO: keep track of ref count and sync status in the storage, without adding it to the in memory
// representation.

#[derive(Debug, Clone)]
/// In memory reprsentation of treap node.
pub(crate) struct Node {
    /// The hash of this node, uniquely identifying its key, value, and children.
    hash: Hash,

    // Key value
    key: Box<[u8]>,
    value: Hash,

    // Rank
    rank: Hash,

    // Children
    left: Option<Hash>,
    right: Option<Hash>,
}

#[derive(Debug)]
pub(crate) enum Branch {
    Left,
    Right,
}

impl Node {
    pub fn new(key: &[u8], value: Hash) -> Self {
        let mut hasher = Hasher::new();
        hasher.update(key);

        let rank = hasher.finalize();

        let mut node = Self {
            hash: EMPTY_HASH,

            key: key.into(),
            value,
            left: None,
            right: None,
            rank,
        };

        node
    }

    // TODO: add from bytes and remember to update its hash.

    // === Getters ===

    pub(crate) fn key(&self) -> &[u8] {
        &self.key
    }

    pub(crate) fn value(&self) -> &Hash {
        &self.value
    }

    pub(crate) fn rank(&self) -> &Hash {
        &self.rank
    }

    /// Returns the hash of the node.
    pub(crate) fn hash(&self) -> &Hash {
        &self.hash
    }

    pub(crate) fn left(&self) -> &Option<Hash> {
        &self.left
    }

    pub(crate) fn right(&self) -> &Option<Hash> {
        &self.right
    }

    // === Private Methods ===

    pub(crate) fn update_hash(&mut self) -> Hash {
        let mut hasher = Hasher::new();

        hasher.update(&self.key);
        hasher.update(self.value.as_bytes());
        hasher.update(self.left.unwrap_or(EMPTY_HASH).as_bytes());
        hasher.update(self.right.unwrap_or(EMPTY_HASH).as_bytes());

        self.hash = hasher.finalize();
        self.hash
    }

    /// When inserting a node, once we find its instertion point,
    /// we give one of its children (depending on the direction),
    /// to the current node at the insertion position, and then we
    /// replace that child with the updated current node.
    pub(crate) fn insertion_swap(
        &mut self,
        direction: Branch,
        current_node: &mut Node,
        storage: &mut MemoryStorage,
    ) {
        match direction {
            Branch::Left => current_node.set_child(&Branch::Left, *self.right()),
            Branch::Right => current_node.set_child(&Branch::Left, *self.left()),
        }

        current_node.update(storage);

        match direction {
            Branch::Left => self.left = Some(*current_node.hash()),
            Branch::Right => self.right = Some(*current_node.hash()),
        }

        self.update(storage);
    }

    pub(crate) fn set_child(&mut self, branch: &Branch, hash: Option<Hash>) {
        // decrement old child's ref count.

        // set children
        match branch {
            Branch::Left => self.left = hash,
            Branch::Right => self.right = hash,
        }

        // TODO: increment node's ref count.
    }

    pub(crate) fn update(&mut self, storage: &mut MemoryStorage) -> &Hash {
        // TODO: save new hash to storage.
        // TODO: increment ref count.
        // TODO: decrement ref count of old hash!

        // let old_hash = self.hash();

        self.update_hash();

        storage.insert_node(self);

        self.hash()
    }
}
