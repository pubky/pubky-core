use redb::{Database, ReadableTable, Table, TableDefinition, WriteTransaction};

use crate::{Hash, Hasher};

// TODO: Are we creating too many hashers?
// TODO: are we calculating the rank and hash too often?

const HASH_LEN: usize = 32;

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

        let (right, rest) = decode(rest);
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

    pub(crate) fn set_child(
        &mut self,
        branch: &Branch,
        new_child: Option<Hash>,
        table: &mut Table<&[u8], (u64, &[u8])>,
    ) {
        let old_child = match branch {
            Branch::Left => self.left,
            Branch::Right => self.right,
        };

        // increment old child's ref count.
        decrement_ref_count(old_child, table);

        // increment new child's ref count.
        increment_ref_count(new_child, table);

        // set new child
        match branch {
            Branch::Left => self.left = new_child,
            Branch::Right => self.right = new_child,
        }

        self.save(table);
    }

    pub(crate) fn save(&self, table: &mut Table<&[u8], (u64, &[u8])>) {
        let encoded = self.canonical_encode();
        let hash = hash(&encoded);

        table.insert(
            hash.as_bytes().as_slice(),
            (self.ref_count, encoded.as_slice()),
        );
    }

    // === Private Methods ===

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

enum RefCountDiff {
    Increment,
    Decrement,
}

fn increment_ref_count(child: Option<Hash>, table: &mut Table<&[u8], (u64, &[u8])>) {
    update_ref_count(child, RefCountDiff::Increment, table);
}

fn decrement_ref_count(child: Option<Hash>, table: &mut Table<&[u8], (u64, &[u8])>) {
    update_ref_count(child, RefCountDiff::Decrement, table);
}

fn update_ref_count(
    child: Option<Hash>,
    ref_diff: RefCountDiff,
    table: &mut Table<&[u8], (u64, &[u8])>,
) {
    if let Some(hash) = child {
        let mut existing = table
            .get(hash.as_bytes().as_slice())
            .unwrap()
            .expect("Child shouldn't be messing!");

        let (ref_count, bytes) = {
            let (r, v) = existing.value();
            (r + 1, v.to_vec())
        };
        drop(existing);

        let ref_count = match ref_diff {
            RefCountDiff::Increment => ref_count + 1,
            RefCountDiff::Decrement => {
                if ref_count > 0 {
                    ref_count - 1
                } else {
                    ref_count
                }
            }
        };

        match ref_count {
            0 => {
                // TODO: This doesn't seem to work yet.
                // I think we should keep doing it recursively.
                // or wait for the GC to do it?
                // TODO: Is it the case that we don't clean up the other branch when the tree requires that?
                // Well that should not happen really, but it is probably caused by the fact that
                // the order of keys are missed up (not history independent)
                //
                // TODO: Confirm (read: test) this, because it is not easy to see in graphs.
                table.remove(hash.as_bytes().as_slice());
            }
            _ => {
                table.insert(hash.as_bytes().as_slice(), (ref_count, bytes.as_slice()));
            }
        }
    }
}
