//! #![doc = include_str!("../README.md")]
//! **Pubky SDK** a small, ergonomic library for building Pubky applications.
//!
//! # Quick start
//! ```no_run
//! use pubky::prelude::*;
//!
//! # async fn run() -> pubky::Result<()> {
//! // Create a signer (holds your keypair). You can also persist/import your own keys.
//! let signer = PubkySigner::new(Keypair::random())?;
//!
//! // Sign up on a homeserver (identified by its public key)
//! let homeserver = PublicKey::try_from("o4dksf...uyy").unwrap();
//! let agent = signer.signup_agent(&homeserver, None).await?;
//!
//! // Read/write using the drive API (session-scoped)
//! agent.drive().put("/pub/app/hello.txt", "hello").await?;
//! let body = agent.drive().get("/pub/app/hello.txt").await?.bytes().await?;
//! assert_eq!(&body, b"hello");
//!
//! // Unauthenticated read (public): user-qualified path, no session required
//! let public_drive = PubkyDrive::public()?;
//! let txt = public_drive
//!     .get(format!("{}/pub/app/hello.txt", signer.pubky()))
//!     .await?
//!     .text()
//!     .await?;
//! assert_eq!(txt, "hello");
//!
//! // Publish or resolve your homeserver `_pubky` (PkDNS/PKARR) record
//! signer.pkdns().publish_homeserver_if_stale(None).await?;
//! let resolved = Pkdns::new()?.get_homeserver(&signer.pubky()).await;
//! println!("current homeserver: {:?}", resolved);
//!
//! // Keyless third-party app: start PubkyPairingAuth and turn it into an agent
//! let capabilities = Capabilities::builder().write("/pub/pubky.app/").finish();
//! let (sub, url) = PubkyPairingAuth::new(None, &capabilities)?.subscribe();  // None for default relay.
//! // display `url` via QR or deeplink it so the Signer can send the auth token.
//! // signer.approve_pubkyauth_request(url);
//! let agent = sub.into_agent().await?; // session-bound agent
//! # Ok(()) }
//! ```

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
pub use pkdns::core::Pkdns;
pub use signer::PubkySigner;

// Export common types and constants
pub use crate::drive::{
    list::ListBuilder,
    path::{FilePath, PubkyPath},
};
pub use pkarr::DEFAULT_RELAYS;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    capabilities::{Capabilities, Capability},
    recovery_file,
    session::Session,
};
