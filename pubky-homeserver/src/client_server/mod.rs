//! Main data-serving HTTP server.
//!
//! Listens on two sockets: plain HTTP (ICANN) and
//! TLS using the server's Ed25519 keypair.
//! Routes are organized into server-level endpoints and per-tenant endpoints.

mod app;
pub(crate) mod app_state;
mod extractors;
mod layers;
pub(crate) mod routes;

pub use app::create_app;
pub use app::{ClientServer, ClientServerBuildError};
pub(crate) use app_state::AppState;
