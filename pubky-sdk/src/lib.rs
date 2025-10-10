#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

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
    resource::{IntoPubkyResource, IntoResourcePath, resolve_pubky},
    resource::{PubkyResource, ResourcePath},
    stats::ResourceStats,
};
pub use actors::auth_flow::DEFAULT_HTTP_RELAY;
pub use actors::pkdns::DEFAULT_STALE_AFTER;
#[doc(inline)]
pub use pkarr::DEFAULT_RELAYS;

// Re-exports
#[doc(inline)]
pub use pkarr::{Keypair, PublicKey};
#[doc(inline)]
pub use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    recovery_file,
};
#[doc(inline)]
pub use reqwest::{Method, StatusCode};
