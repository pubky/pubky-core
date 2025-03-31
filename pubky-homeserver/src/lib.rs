#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod admin;
mod app_context;
mod constants;
mod core;
mod data_directory;
mod homeserver_suite;
mod persistence;

pub use admin::AdminServer;
pub use app_context::AppContext;
pub use core::HomeserverCore;
pub use data_directory::*;
pub use homeserver_suite::HomeserverSuite;
