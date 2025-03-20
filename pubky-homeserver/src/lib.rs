#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod config;
mod core;
mod io;
mod config2;

pub use config2::*;
pub use io::Homeserver;
pub use io::HomeserverBuilder;
