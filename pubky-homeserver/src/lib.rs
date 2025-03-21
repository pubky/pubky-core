#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod config_old;
mod core;
mod homeserver;
mod data_dir;

pub use data_dir::*;
pub use homeserver::Homeserver;
pub use homeserver::HomeserverBuilder;
