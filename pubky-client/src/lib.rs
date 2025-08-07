//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod api;
mod client;
mod errors;
mod internal;
mod macros;

// --- PUBLIC API EXPORTS ---
pub use client::{Client, ClientBuilder};
pub use errors::{BuildError, Error};
// Export common types.
pub use api::{auth::AuthRequest, public::ListBuilder};
// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::recovery_file;
