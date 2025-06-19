//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

pub mod errors;
pub mod native;
mod types;
mod utils;

pub use crate::native::Client;
pub use crate::native::{ClientBuilder, api::auth::AuthRequest, api::public::ListBuilder};

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::recovery_file;

#[cfg(wasm_browser)]
mod wasm;
#[cfg(wasm_browser)]
pub use wasm::constructor::PubkyClient;
