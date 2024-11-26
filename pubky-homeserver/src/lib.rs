pub mod config;
mod core;
mod database;
mod error;
mod extractors;
mod pkarr;
mod routes;
mod server;

pub use core::HomeserverCore;
pub use server::Homeserver;
