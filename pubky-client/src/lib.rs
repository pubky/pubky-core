//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod agent;
mod api;
mod client;
pub mod errors;
mod internal;
mod macros;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
pub use agent::PubkyAgent;
pub use client::{PubkyClient, PubkyClientBuilder};
pub use errors::{BuildError, Error, Result};
// Export common types.
pub use api::{auth::AuthRequest, public::ListBuilder};
// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    capabilities::{Capabilities, Capability},
    recovery_file,
};

// --- Back-compat aliases (soft-deprecated) ---
#[allow(deprecated)]
pub type Client = PubkyClient;
#[allow(deprecated)]
pub type ClientBuilder = PubkyClientBuilder;
