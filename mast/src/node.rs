//! In memory representation of a treap node.

use redb::{ReadableTable, Table};

use crate::{Hash, Hasher, HASH_LEN};

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
    rank: Hash,
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

            rank: hash(key),
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

    pub fn rank(&self) -> &Hash {
        &self.rank
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

    /// Encodes the node in a canonical way:
    /// - 1 byte header
    ///     - 0b1100_0000: Two reserved bits
    ///     - 0b0011_0000: Two bits represents the size of the key length (0, u8, u16, u32)
    ///     - 0b0000_1100: Two bits represents the size of the value length (0, u8, u16, u32)
    ///     - 0b0000_0010: left child is present
    ///     - 0b0000_0001: right child is present
    /// - key
    /// - value
    fn canonical_encode(&self) -> Vec<u8> {
        let key_length = self.key.len();
        let val_length = self.value.len();

        let key_length_encoding_length = len_encoding_length(key_length);
        let val_length_encoding_length = len_encoding_length(val_length);

        let header = (key_length_encoding_length << 4)
            | (val_length_encoding_length << 2)
            | ((self.left.is_some() as u8) << 1)
            | (self.right.is_some() as u8);

        let mut bytes = vec![header];

        // Encode key length
        match key_length_encoding_length {
            1 => bytes.push(key_length as u8),
            2 => bytes.extend_from_slice(&(key_length as u16).to_be_bytes()),
            3 => bytes.extend_from_slice(&(key_length as u32).to_be_bytes()),
            _ => {} // Do nothing for 0 length
        }

        // Encode value length
        match val_length_encoding_length {
            1 => bytes.push(val_length as u8),
            2 => bytes.extend_from_slice(&(val_length as u16).to_be_bytes()),
            3 => bytes.extend_from_slice(&(val_length as u32).to_be_bytes()),
            _ => {} // Do nothing for 0 length
        }

        bytes.extend_from_slice(&self.key);
        bytes.extend_from_slice(&self.value);

        if let Some(left) = &self.left {
            bytes[0] |= 0b0000_0010;
            bytes.extend_from_slice(left.as_bytes());
        }
        if let Some(right) = &self.right {
            bytes[0] |= 0b0000_0001;
            bytes.extend_from_slice(right.as_bytes());
        }

        bytes
    }
}

fn hash(bytes: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);

    hasher.finalize()
}

fn decode_node(data: (u64, &[u8])) -> Node {
    let (ref_count, encoded_node) = data;

    // We can calculate the size of then node from the first few bytes.
    let header = encoded_node[0];

    let mut rest = &encoded_node[1..];

    let key_length = match (header & 0b0011_0000) >> 4 {
        1 => {
            let len = rest[0] as usize;
            rest = &rest[1..];
            len
        }
        2 => {
            let len = u16::from_be_bytes(rest[0..3].try_into().unwrap()) as usize;
            rest = &rest[3..];
            len
        }
        3 => {
            let len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
            rest = &rest[4..];
            len
        }
        _ => 0,
    };

    let val_length = match (header & 0b0000_1100) >> 2 {
        1 => {
            let len = rest[0] as usize;
            rest = &rest[1..];
            len
        }
        2 => {
            let len = u16::from_be_bytes(rest[0..3].try_into().unwrap()) as usize;
            rest = &rest[3..];
            len
        }
        3 => {
            let len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
            rest = &rest[4..];
            len
        }
        _ => 0,
    };

    let key = &rest[..key_length];
    rest = &rest[key_length..];

    let value = &rest[..val_length];
    rest = &rest[val_length..];

    let left = match header & 0b0000_0010 == 0 {
        true => None,
        false => {
            let hash_bytes: [u8; HASH_LEN] = rest[0..32].try_into().unwrap();
            rest = &rest[32..];

            Some(Hash::from_bytes(hash_bytes))
        }
    };

    let right = match header & 0b0000_0001 == 0 {
        true => None,
        false => {
            let hash_bytes: [u8; HASH_LEN] = rest[0..32].try_into().unwrap();
            Some(Hash::from_bytes(hash_bytes))
        }
    };

    Node {
        key: key.into(),
        value: value.into(),
        left,
        right,

        ref_count,

        rank: hash(key),
        hash: None,
    }
}

fn len_encoding_length(len: usize) -> u8 {
    if len == 0 {
        0
    } else if len <= u8::max_value() as usize {
        1
    } else if len <= u16::max_value() as usize {
        2
    } else {
        3
    }
}
