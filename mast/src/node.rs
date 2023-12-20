//! In memory representation of a treap node.

use redb::{ReadableTable, Table};

use crate::{Hash, Hasher, HASH_LEN};

// TODO: Are we creating too many hashers?
// TODO: are we calculating the rank and hash too often?
// TODO: remove unused
// TODO: remove unwrap

#[derive(Debug, Clone)]
/// In memory reprsentation of treap node.
pub(crate) struct Node {
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
    pub fn new(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            left: None,
            right: None,

            ref_count: 0,
        }
    }

    pub fn decode(data: (u64, &[u8])) -> Node {
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

    // === Getters ===

    pub(crate) fn key(&self) -> &[u8] {
        &self.key
    }

    pub(crate) fn value(&self) -> &[u8] {
        &self.value
    }

    pub(crate) fn left(&self) -> &Option<Hash> {
        &self.left
    }

    pub(crate) fn right(&self) -> &Option<Hash> {
        &self.right
    }

    // === Public Methods ===

    pub(crate) fn rank(&self) -> Hash {
        hash(&self.key)
    }

    /// Returns the hash of the node.
    pub(crate) fn hash(&self) -> Hash {
        hash(&self.canonical_encode())
    }

    pub(crate) fn decrement_ref_count(&self, table: &mut Table<&[u8], (u64, &[u8])>) {}

    pub(crate) fn save(&self, table: &mut Table<&[u8], (u64, &[u8])>) {
        let encoded = self.canonical_encode();
        let hash = hash(&encoded);

        table.insert(
            hash.as_bytes().as_slice(),
            (self.ref_count, encoded.as_slice()),
        );
    }

    // === Private Methods ===

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
        };
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

/// Returns the node for a given hash.
pub(crate) fn get_node<'a>(
    table: &'a impl ReadableTable<&'static [u8], (u64, &'static [u8])>,
    hash: &[u8],
) -> Option<Node> {
    let existing = table.get(hash).unwrap();

    if existing.is_none() {
        return None;
    }
    let data = existing.unwrap();

    Some(Node::decode(data.value()))
}

/// Returns the root hash for a given table.
pub(crate) fn get_root_hash<'a>(
    table: &'a impl ReadableTable<&'static [u8], &'static [u8]>,
    name: &str,
) -> Option<Hash> {
    let existing = table.get(name.as_bytes()).unwrap();
    if existing.is_none() {
        return None;
    }
    let hash = existing.unwrap();

    let hash: [u8; HASH_LEN] = hash.value().try_into().expect("Invalid root hash");
    Some(Hash::from_bytes(hash))
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
    let rest = &remaining[value.len() as usize..];

    (value, rest)
}

fn hash(bytes: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);

    hasher.finalize()
}
