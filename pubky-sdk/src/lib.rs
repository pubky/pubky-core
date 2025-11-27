#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::multiple_crate_versions,
    reason = "workspace dependencies still require distinct versions"
)]
#![cfg_attr(
    target_arch = "wasm32",
    allow(clippy::future_not_send, reason = "WASM futures are single-threaded")
)]

mod pubky;

mod actors;
mod client;
pub mod errors;
mod macros;

mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
// SDK facade
#[doc(inline)]
pub use pubky::Pubky;
// Transport
#[doc(inline)]
pub use client::core::{PubkyHttpClient, PubkyHttpClientBuilder};
// High level actors
#[doc(inline)]
pub use actors::Pkdns;
#[doc(inline)]
pub use actors::PubkyAuthFlow;
#[doc(inline)]
pub use actors::PubkySession;
#[doc(inline)]
pub use actors::PubkySigner;
#[doc(inline)]
pub use actors::{PublicStorage, SessionStorage};

// Error and global client
#[doc(inline)]
pub use errors::{BuildError, Error, Result};

// Export common types and constants
#[doc(inline)]
pub use crate::actors::storage::{
    list::ListBuilder,
    resource::{IntoPubkyResource, IntoResourcePath, resolve_pubky},
    resource::{PubkyResource, ResourcePath},
    stats::ResourceStats,
};
#[doc(inline)]
pub use actors::auth_flow::DEFAULT_HTTP_RELAY;
#[doc(inline)]
pub use actors::pkdns::DEFAULT_STALE_AFTER;
#[doc(inline)]
pub use pkarr::DEFAULT_RELAYS;

// Re-exports
#[doc(inline)]
pub use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    crypto::{Keypair, PublicKey},
    recovery_file,
};
pub use reqwest::{Method, StatusCode};
