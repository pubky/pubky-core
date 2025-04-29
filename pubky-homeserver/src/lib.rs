//! Homeserver for Pubky
//!
//! This crate provides a homeserver for Pubky. It is responsible for handling user authentication,
//! authorization, and other core functionalities.
//!
//! This crate is part of the Pubky project.
//!
//! For more information, see the [Pubky project](https://github.com/pubky/pubky).

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
mod shared;

pub use admin::{AdminServer, AdminServerBuildError};
pub use app_context::{AppContext, AppContextConversionError};
pub use core::{HomeserverBuildError, HomeserverCore};
pub use data_directory::*;
pub use homeserver_suite::{HomeserverSuite, HomeserverSuiteBuildError};
