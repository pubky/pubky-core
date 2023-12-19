use redb::{Database, ReadableTable, Table, TableDefinition, WriteTransaction};

use crate::{Hash, Hasher, EMPTY_HASH};

// TODO: Are we creating too many hashers?
// TODO: are we calculating the rank and hash too often?

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
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let (size, remaining) = varu64::decode(bytes).unwrap();
        let key = remaining[..size as usize].to_vec().into_boxed_slice();

        let (size, remaining) = varu64::decode(&remaining[size as usize..]).unwrap();
        let value = remaining[..size as usize].to_vec().into_boxed_slice();

        let left = remaining[size as usize..((size as usize) + 32)]
            .try_into()
            .map_or(None, |h| Some(Hash::from_bytes(h)));

        let right = remaining[(size as usize) + 32..((size as usize) + 32 + 32)]
            .try_into()
            .map_or(None, |h| Some(Hash::from_bytes(h)));

        Node {
            key,
            value,
            left,
            right,

            ref_count: 0,
        }
    }

    pub fn new(key: &[u8], value: &[u8]) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            left: None,
            right: None,

            ref_count: 0,
        }
    }
    // TODO: remember to update its hash.

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

        let encoded = self.canonical_encode();
        table.insert(
            hash(&encoded).as_bytes().as_slice(),
            (self.ref_count, encoded.as_slice()),
        );
    }

    // === Private Methods ===

    fn canonical_encode(&self) -> Vec<u8> {
        let mut bytes = vec![];

        encode(&self.key, &mut bytes);
        encode(&self.value, &mut bytes);
        encode(
            &self.left.map(|h| h.as_bytes().to_vec()).unwrap_or_default(),
            &mut bytes,
        );
        encode(
            &self.left.map(|h| h.as_bytes().to_vec()).unwrap_or_default(),
            &mut bytes,
        );

        bytes
    }
}

fn encode(bytes: &[u8], out: &mut Vec<u8>) {
    varu64::encode(bytes.len() as u64, out);
    out.extend_from_slice(bytes);
}

fn hash(bytes: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);

    hasher.finalize()
}

fn increment_ref_count(child: Option<Hash>, table: &mut Table<&[u8], (u64, &[u8])>) {
    update_ref_count(child, 1, table);
}

fn decrement_ref_count(child: Option<Hash>, table: &mut Table<&[u8], (u64, &[u8])>) {
    update_ref_count(child, -1, table);
}

fn update_ref_count(child: Option<Hash>, ref_diff: i8, table: &mut Table<&[u8], (u64, &[u8])>) {
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

        table.insert(
            hash.as_bytes().as_slice(),
            (ref_count + ref_diff as u64, bytes.as_slice()),
        );
    }
}
