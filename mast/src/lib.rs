#![allow(unused)]

pub mod db;
pub mod error;
mod node;
mod operations;

#[cfg(test)]
mod test;

pub(crate) use blake3::{Hash, Hasher};
pub(crate) const HASH_LEN: usize = 32;

pub use db::Database;
pub use error::Error;

// Alias Result to be the crate Result.
pub type Result<T, E = Error> = core::result::Result<T, E>;
