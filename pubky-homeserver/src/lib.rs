#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod core;
mod persistence;
mod data_directory;
mod admin;
mod homeserver;
pub mod app_context;
pub(crate) mod constants;

pub use data_directory::*;
pub use homeserver::HomeserverSuite;
