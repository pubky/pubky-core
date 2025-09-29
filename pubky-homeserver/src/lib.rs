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

mod admin_server;
mod app_context;
mod client_server;
mod constants;
mod data_directory;
mod homeserver_app;
mod persistence;
mod republishers;
mod shared;
pub mod tracing;

pub use admin_server::{AdminServer, AdminServerBuildError};
pub use app_context::{AppContext, AppContextConversionError};
pub use client_server::{ClientServer, ClientServerBuildError};
pub use data_directory::*;
pub use homeserver_app::{HomeserverApp, HomeserverAppBuildError};
pub use persistence::sql::ConnectionString;
