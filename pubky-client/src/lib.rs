//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

pub mod api;
mod client;
pub mod http_client;
pub mod internal;
#[cfg(not(target_arch = "wasm32"))]
mod native;

// --- PUBLIC API EXPORTS ---

// Export the generic base client for advanced users or other platforms (e.g. wasm in bindings/js)
pub use client::BaseClient;
// Export the configuration object.
pub use client::ClientConfig;

// Conditionally export the easy-to-use native `Client` for native rust users.
// When a user on a native target writes `use pubky::Client`, this is what they will get.
#[cfg(not(target_arch = "wasm32"))]
pub use native::client::Client;

// Export common types.
pub use api::{auth::AuthRequest, public::ListBuilder};
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::recovery_file;

// Export error type.
pub use client::BuildError;
