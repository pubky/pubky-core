#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod agent;
mod auth;
mod client;
mod drive;
pub mod errors;
pub mod global;
mod macros;
mod pkdns;
mod signer;
mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
pub use agent::core::PubkyAgent;
pub use auth::PubkyPairingAuth;
pub use client::core::{PubkyHttpClient, PubkyHttpClientBuilder};
pub use drive::core::PubkyDrive;
pub use errors::{BuildError, Error, Result};
pub use pkdns::Pkdns;
pub use signer::PubkySigner;

// Export common types and constants
pub use crate::drive::{list::ListBuilder, resource::IntoPubkyResource, resource::PubkyResource};
pub use auth::AuthSubscription;
pub use auth::DEFAULT_HTTP_RELAY;
pub use pkarr::DEFAULT_RELAYS;
pub use pkdns::DEFAULT_STALE_AFTER;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    capabilities::{Capabilities, Capability},
    recovery_file,
    session::Session,
};
