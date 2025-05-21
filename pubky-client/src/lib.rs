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

mod shared;
pub mod native;
#[cfg(wasm_browser)]
mod wasm;

#[cfg(not(wasm_browser))]
pub use crate::native::Client;
pub use crate::native::{api::auth::AuthRequest, api::public::ListBuilder, ClientBuilder};

#[cfg(wasm_browser)]
pub use native::Client as NativeClient;
#[cfg(wasm_browser)]
pub use wasm::constructor::Client;

// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::recovery_file;
pub use pubky_common;

pub mod errors {
    pub use super::*;

    #[cfg(not(wasm_browser))]
    pub use native::BuildError;
}
