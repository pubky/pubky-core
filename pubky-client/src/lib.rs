//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

pub mod api;
mod client;
pub mod internal;
mod macros;
pub use client::*;

pub use api::{auth::AuthRequest, public::ListBuilder};
pub mod errors;
pub use client::Client;
pub use client::ClientBuilder;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::recovery_file;

pub mod errors {
    pub use super::*;
    pub use BuildError;
}
