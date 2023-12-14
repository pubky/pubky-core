//! Zip tree node.

use blake3::{Hash, Hasher};
use bytes::{BufMut, Bytes, BytesMut};

pub const HASH_LEN: usize = blake3::OUT_LEN;
pub const EMPTY_HASH: Hash = Hash::from_bytes([0_u8; HASH_LEN]);

#[derive(Debug)]
/// A serialized node.
pub struct Node {
    key: String,
    value: Hash,
    left: Option<Hash>,
    right: Option<Hash>,
}

impl Node {
    pub fn new(key: String, value: Hash) -> Self {
        Self {
            key,
            value,
            left: None,
            right: None,
        }
    }

    // === Getters ===

    // pub fn key(&self) -> &String {
    //     &self.key
    // }

    // pub fn left(&self) -> Option<Hash> {
    //     self.left
    // }

    // === Public Methods ===
    //
    pub fn serialize(&self) -> Bytes {
        let mut bytes = BytesMut::new();

        let mut header = 0_u8;
        if self.left.is_some() {
            header |= 0b0000_0010;
        }
        if self.right.is_some() {
            header |= 0b0000_0001;
        }
        bytes.put_u8(0_u8);

        // Encode the key
        let mut key_len = [0_u8; 9];
        let size = varu64::encode(self.key.len() as u64, &mut key_len);
        bytes.extend_from_slice(&key_len[..size]);
        bytes.extend_from_slice(self.key.as_bytes());

        // Encode the value
        bytes.extend_from_slice(self.value.as_bytes());

        if let Some(left) = self.left {
            bytes.extend_from_slice(left.as_bytes());
        }
        if let Some(right) = self.right {
            bytes.extend_from_slice(right.as_bytes());
        }

        bytes.freeze()
    }

    // pub fn hash(&self) -> Hash {
    //     let mut hasher = Hasher::new();
    //     hasher.update(&self.serialize());
    //     hasher.finalize().into()
    // }

    pub fn deserialize(encoded: &Bytes) -> Result<Node, ()> {
        let header = encoded.first().ok_or(())?;

        let (n, rest) = varu64::decode(&encoded[1..]).map_err(|_| ())?;
        let key_len = n as usize;
        let key = String::from_utf8(rest[..key_len].to_vec()).map_err(|_| ())?;
        let value = Hash::from_bytes(
            rest[key_len..key_len + HASH_LEN]
                .try_into()
                .map_err(|_| ())?,
        );

        let mut left: Option<Hash> = None;
        let mut right: Option<Hash> = None;

        if *header & 0b0000_0010 != 0 {
            let start = key_len + HASH_LEN;
            let end = start + HASH_LEN;

            left = Some(Hash::from_bytes(
                rest[start..end].try_into().map_err(|_| ())?,
            ))
        }
        if *header & 0b0000_0001 != 0 {
            let start = key_len + HASH_LEN + if left.is_some() { HASH_LEN } else { 0 };
            let end = start + HASH_LEN;

            right = Some(Hash::from_bytes(
                rest[start..end].try_into().map_err(|_| ())?,
            ))
        }

        Ok(Self {
            key,
            value,
            left,
            right,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn encode() {
        let node = Node::new("key".to_string(), EMPTY_HASH);

        let encoded = node.serialize();
        let decoded = Node::deserialize(&encoded);
    }
}
