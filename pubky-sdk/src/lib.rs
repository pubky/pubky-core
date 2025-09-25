#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod auth_flow;
mod client;
pub mod errors;
mod global;
mod macros;
mod pkdns;
mod session;
mod signer;
mod storage;
mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
// Transport
pub use client::core::{PubkyHttpClient, PubkyHttpClientBuilder};
// High level actors
pub use auth_flow::PubkyAuthFlow;
pub use pkdns::Pkdns;
pub use session::core::PubkySession;
pub use signer::PubkySigner;
pub use storage::core::{PublicStorage, SessionStorage};

// Error and global client
pub use errors::{BuildError, Error, Result};
pub use global::{drop_global_client, global_client, set_global_client};

// Export common types and constants
pub use crate::storage::{
    list::ListBuilder,
    resource::{IntoPubkyResource, IntoResourcePath},
    resource::{PubkyResource, ResourcePath},
    stats::ResourceStats,
};
pub use auth_flow::DEFAULT_HTTP_RELAY;
pub use pkarr::DEFAULT_RELAYS;
pub use pkdns::DEFAULT_STALE_AFTER;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    recovery_file,
};
pub use reqwest::{Method, StatusCode};
