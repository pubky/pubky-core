// #![allow(unused)]
pub mod crypto;
pub mod error;

// Re-exports
pub use error::Error;

// Alias Result to be the crate Result.
pub type Result<T, E = Error> = core::result::Result<T, E>;
