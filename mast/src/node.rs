//! In memory representation of a treap node.

use redb::{ReadableTable, Table};

use crate::{Hash, Hasher, HASH_LEN};

// TODO: room for improvement (pending actual benchmarks to justify):
//  - cache encoding

// TODO: remove unwrap
// TODO: KeyType and ValueType

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

    // Memoized hashes
    /// The Hash of the node, if None then something changed, and the hash should be recomputed.
    hash: Option<Hash>,
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
    pub(crate) fn new(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            left: None,
            right: None,

            ref_count: 0,
            hash: None,
        }
    }

    pub(crate) fn open(
        table: &'_ impl ReadableTable<&'static [u8], (u64, &'static [u8])>,
        hash: Hash,
    ) -> Option<Node> {
        // TODO: make it Result instead!
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

    pub fn rank(&self) -> Hash {
        hash(self.key())
    }

    pub(crate) fn ref_count(&self) -> &u64 {
        &self.ref_count
    }

    /// Returns the hash of the node.
    pub fn hash(&mut self) -> Hash {
        self.hash.unwrap_or_else(|| {
            let encoded = self.canonical_encode();
            let hash = hash(&encoded);
            self.hash = Some(hash);
            hash
        })
    }

    // === Private Methods ===

    /// Set the value.
    pub(crate) fn set_value(&mut self, value: &[u8]) -> &mut Self {
        self.value = value.into();
        self.hash = None;

        self
    }

    /// Set the left child, save the updated node, and return the new hash.
    pub(crate) fn set_left_child(&mut self, child: Option<Hash>) -> &mut Self {
        self.set_child(Branch::Left, child)
    }

    /// Set the right child, save the updated node, and return the new hash.
    pub(crate) fn set_right_child(&mut self, child: Option<Hash>) -> &mut Self {
        self.set_child(Branch::Right, child)
    }

    /// Set the child, update its ref_count, save the updated node and return it.
    fn set_child(&mut self, branch: Branch, new_child: Option<Hash>) -> &mut Self {
        match branch {
            Branch::Left => self.left = new_child,
            Branch::Right => self.right = new_child,
        };
        self.hash = None;

        self
    }

    pub(crate) fn increment_ref_count(&mut self) -> &mut Self {
        self.update_ref_count(RefCountDiff::Increment)
    }

    pub(crate) fn decrement_ref_count(&mut self) -> &mut Self {
        self.update_ref_count(RefCountDiff::Decrement)
    }

    fn update_ref_count(&mut self, diff: RefCountDiff) -> &mut Self {
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

        // We only updaet the ref count, and handle deletion elsewhere.
        self.ref_count = ref_count;
        self
    }

    /// Saves the node to the nodes table by its hash.
    pub(crate) fn save(&mut self, table: &mut Table<&[u8], (u64, &[u8])>) -> &mut Self {
        // TODO: keep data in encoded in a bytes field.
        let encoded = self.canonical_encode();

        table
            .insert(
                hash(&encoded).as_bytes().as_slice(),
                (self.ref_count, encoded.as_slice()),
            )
            .unwrap();

        self
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

pub(crate) fn hash(bytes: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);

    hasher.finalize()
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

fn decode_node(data: (u64, &[u8])) -> Node {
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
        hash: None,
    }
}
