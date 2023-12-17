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

pub(crate) enum Child {
    Left,
    Right,
}

impl Node {
    // TODO: Convert to Result, since it shouldn't be missing!
    pub(crate) fn open(storage: &MemoryStorage, hash: Hash) -> Option<Self> {
        storage.get_node(&hash)
    }

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

        node.update_hash();

        node
    }

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

    // /// Replace a child of this node, and return the old child.
    // ///
    // /// This method decrements the ref count of the old child,
    // /// and incrments the ref count of the new child,
    // ///
    // /// but it dosn't flush any changes to the storage.
    // pub(crate) fn set_child(
    //     &mut self,
    //     node: &mut Option<Node>,
    //     child: Child,
    //     storage: &MemoryStorage,
    // ) -> Option<Node> {
    //     // Decrement old child's ref count.
    //     let mut old_child = match child {
    //         Child::Left => self.left,
    //         Child::Right => self.right,
    //     }
    //     .and_then(|hash| storage.get_node(&hash));
    //     old_child.as_mut().map(|n| n.decrement_ref_count());
    //
    //     // Increment new child's ref count.
    //     node.as_mut().map(|n| n.increment_ref_count());
    //
    //     // swap children
    //     match child {
    //         Child::Left => self.left = node.as_mut().map(|n| n.update_hash()),
    //         Child::Right => self.right = node.as_mut().map(|n| n.update_hash()),
    //     }
    //
    //     // Update this node's hash.
    //     self.update_hash();
    //
    //     old_child
    // }

    pub(crate) fn set_child_hash(&mut self, child: Child, hash: Hash) {
        // Swap the child.
        match child {
            Child::Left => self.left = Some(hash),
            Child::Right => self.right = Some(hash),
        }

        // Update this node's hash, after updating the child.
        self.update_hash();
    }
}
