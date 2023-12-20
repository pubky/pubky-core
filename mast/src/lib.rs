#![allow(unused)]

mod mermaid;
mod node;
mod operations;
pub mod treap;

pub(crate) use blake3::{Hash, Hasher};

pub const HASH_LEN: usize = 32;
