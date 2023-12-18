#![allow(unused)]

mod mermaid;
mod node;
mod storage;
pub mod treap;

pub(crate) use blake3::{Hash, Hasher};

pub(crate) use node::Node;
pub(crate) use treap::HashTreap;

// TODO: If we are going to use Iroh Bytes, might as well ues this from Iroh basics.
/// The hash for the empty byte range (`b""`).
pub(crate) const EMPTY_HASH: Hash = Hash::from_bytes([
    175, 19, 73, 185, 245, 249, 161, 166, 160, 64, 77, 234, 54, 220, 201, 73, 155, 203, 37, 201,
    173, 193, 18, 183, 204, 154, 147, 202, 228, 31, 50, 98,
]);
