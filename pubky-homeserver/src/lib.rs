#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod admin;
pub mod app_context;
mod constants;
mod core;
mod data_directory;
mod homeserver;
mod persistence;

pub use data_directory::*;
pub use homeserver::HomeserverSuite;
