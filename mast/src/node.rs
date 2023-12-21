//! In memory representation of a treap node.

use redb::{ReadableTable, Table};

use crate::{Hash, Hasher, HASH_LEN};

// TODO: Are we creating too many hashers?
// TODO: are we calculating the rank and hash too often?
// TODO: remove unwrap

#[derive(Debug, Clone, PartialEq)]
/// In memory reprsentation of treap node.
pub struct Node {
    // Key value
    key: Box<[u8]>,
    value: Box<[u8]>,

    // Children
    left: Option<Hash>,
    right: Option<Hash>,

    // Metadata that should not be encoded.
    ref_count: u64,
}

#[derive(Debug)]
pub(crate) enum Branch {
    Left,
    Right,
}

#[derive(Debug)]
enum RefCountDiff {
    Increment,
    Decrement,
}

impl Node {
    pub(crate) fn open(
        table: &'_ impl ReadableTable<&'static [u8], (u64, &'static [u8])>,
        hash: Hash,
    ) -> Option<Node> {
        let existing = table.get(hash.as_bytes().as_slice()).unwrap();

        existing.map(|existing| {
            let (ref_count, bytes) = {
                let (r, v) = existing.value();
                (r, v.to_vec())
            };
            drop(existing);

            decode_node((ref_count, &bytes))
        })
    }

    pub(crate) fn insert(
        table: &mut Table<&[u8], (u64, &[u8])>,
        key: &[u8],
        value: &[u8],
        left: Option<Hash>,
        right: Option<Hash>,
    ) -> Hash {
        let node = Self {
            key: key.into(),
            value: value.into(),
            left,
            right,

            ref_count: 1,
        };

        let encoded = node.canonical_encode();
        let hash = hash(&encoded);

        table
            .insert(
                hash.as_bytes().as_slice(),
                (node.ref_count, encoded.as_slice()),
            )
            .unwrap();

        hash
    }

    // === Getters ===

    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn value(&self) -> &[u8] {
        &self.value
    }

    pub fn left(&self) -> &Option<Hash> {
        &self.left
    }

    pub fn right(&self) -> &Option<Hash> {
        &self.right
    }

    // === Public Methods ===

    pub fn rank(&self) -> Hash {
        hash(&self.key)
    }

    /// Returns the hash of the node.
    pub fn hash(&self) -> Hash {
        hash(&self.canonical_encode())
    }

    /// Set the value and save the updated node.
    pub(crate) fn set_value(
        &mut self,
        table: &mut Table<&[u8], (u64, &[u8])>,
        value: &[u8],
    ) -> Hash {
        self.value = value.into();
        self.save(table)
    }

    /// Set the left child, save the updated node, and return the new hash.
    pub(crate) fn set_left_child(
        &mut self,
        table: &mut Table<&[u8], (u64, &[u8])>,
        child: Option<Hash>,
    ) -> Hash {
        self.set_child(table, Branch::Left, child)
    }

    /// Set the right child, save the updated node, and return the new hash.
    pub(crate) fn set_right_child(
        &mut self,
        table: &mut Table<&[u8], (u64, &[u8])>,
        child: Option<Hash>,
    ) -> Hash {
        self.set_child(table, Branch::Right, child)
    }

    // === Private Methods ===

    pub fn decrement_ref_count(&self, table: &mut Table<&[u8], (u64, &[u8])>) {
        self.update_ref_count(table, RefCountDiff::Decrement)
    }

    fn set_child(
        &mut self,
        table: &mut Table<&[u8], (u64, &[u8])>,
        branch: Branch,
        child: Option<Hash>,
    ) -> Hash {
        match branch {
            Branch::Left => self.left = child,
            Branch::Right => self.right = child,
        }

        let encoded = self.canonical_encode();
        let hash = hash(&encoded);

        table
            .insert(
                hash.as_bytes().as_slice(),
                (self.ref_count, encoded.as_slice()),
            )
            .unwrap();

        hash
    }

    fn save(&mut self, table: &mut Table<&[u8], (u64, &[u8])>) -> Hash {
        let encoded = self.canonical_encode();
        let hash = hash(&encoded);

        table
            .insert(
                hash.as_bytes().as_slice(),
                (self.ref_count, encoded.as_slice()),
            )
            .unwrap();

        hash
    }

    fn increment_ref_count(&self, table: &mut Table<&[u8], (u64, &[u8])>) {
        self.update_ref_count(table, RefCountDiff::Increment)
    }

    fn update_ref_count(&self, table: &mut Table<&[u8], (u64, &[u8])>, diff: RefCountDiff) {
        let ref_count = match diff {
            RefCountDiff::Increment => self.ref_count + 1,
            RefCountDiff::Decrement => {
                if self.ref_count > 0 {
                    self.ref_count - 1
                } else {
                    self.ref_count
                }
            }
        };

        let bytes = self.canonical_encode();
        let hash = hash(&bytes);

        match ref_count {
            0 => table.remove(hash.as_bytes().as_slice()),
            _ => table.insert(hash.as_bytes().as_slice(), (ref_count, bytes.as_slice())),
        }
        .unwrap();
    }

    fn canonical_encode(&self) -> Vec<u8> {
        let mut bytes = vec![];

        encode(&self.key, &mut bytes);
        encode(&self.value, &mut bytes);

        let left = &self.left.map(|h| h.as_bytes().to_vec()).unwrap_or_default();
        let right = &self
            .right
            .map(|h| h.as_bytes().to_vec())
            .unwrap_or_default();

        encode(left, &mut bytes);
        encode(right, &mut bytes);

        bytes
    }
}

pub(crate) fn rank(key: &[u8]) -> Hash {
    hash(key)
}

fn encode(bytes: &[u8], out: &mut Vec<u8>) {
    // TODO: find a better way to reserve bytes.
    let current_len = out.len();
    for _ in 0..varu64::encoding_length(bytes.len() as u64) {
        out.push(0)
    }
    varu64::encode(bytes.len() as u64, &mut out[current_len..]);

    out.extend_from_slice(bytes);
}

fn decode(bytes: &[u8]) -> (&[u8], &[u8]) {
    let (len, remaining) = varu64::decode(bytes).unwrap();
    let value = &remaining[..len as usize];
    let rest = &remaining[value.len()..];

    (value, rest)
}

fn hash(bytes: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);

    hasher.finalize()
}

pub fn decode_node(data: (u64, &[u8])) -> Node {
    let (ref_count, encoded_node) = data;

    let (key, rest) = decode(encoded_node);
    let (value, rest) = decode(rest);

    let (left, rest) = decode(rest);
    let left = match left.len() {
        0 => None,
        32 => {
            let bytes: [u8; HASH_LEN] = left.try_into().unwrap();
            Some(Hash::from_bytes(bytes))
        }
        _ => {
            panic!("invalid hash length!")
        }
    };

    let (right, _) = decode(rest);
    let right = match right.len() {
        0 => None,
        32 => {
            let bytes: [u8; HASH_LEN] = right.try_into().unwrap();
            Some(Hash::from_bytes(bytes))
        }
        _ => {
            panic!("invalid hash length!")
        }
    };

    Node {
        key: key.into(),
        value: value.into(),
        left,
        right,

        ref_count,
    }
}
