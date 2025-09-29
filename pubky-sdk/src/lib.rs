#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod pubky;

mod actors;
mod client;
pub mod errors;
mod macros;

mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
// SDK facade
pub use pubky::Pubky;
// Transport
pub use client::core::{PubkyHttpClient, PubkyHttpClientBuilder};
// High level actors
pub use actors::Pkdns;
pub use actors::PubkyAuthFlow;
pub use actors::PubkySession;
pub use actors::PubkySigner;
pub use actors::{PublicStorage, SessionStorage};

// Error and global client
pub use errors::{BuildError, Error, Result};

// Export common types and constants
pub use crate::actors::storage::{
    list::ListBuilder,
    resource::{IntoPubkyResource, IntoResourcePath},
    resource::{PubkyResource, ResourcePath},
    stats::ResourceStats,
};
pub use actors::auth_flow::DEFAULT_HTTP_RELAY;
pub use actors::pkdns::DEFAULT_STALE_AFTER;
pub use pkarr::DEFAULT_RELAYS;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    recovery_file,
};
pub use reqwest::{Method, StatusCode};
