//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

macro_rules! cross_debug {
    ($($arg:tt)*) => {
        #[cfg(all(not(test), target_arch = "wasm32"))]
        log::debug!($($arg)*);
        #[cfg(all(not(test), not(target_arch = "wasm32")))]
        tracing::debug!($($arg)*);
        #[cfg(test)]
        println!($($arg)*);
    };
}

pub mod api;
mod client;
pub mod internal;
pub use client::*;

pub use api::{auth::AuthRequest, public::ListBuilder};
pub use client::Client;
pub use client::ClientBuilder;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::recovery_file;

pub mod errors {
    pub use super::*;
    pub use BuildError;
}
