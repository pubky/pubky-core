//! Request middleware for the client server.
//!
//! - [`pubky_host`]: Extracts the tenant public key from the request Host header (TLS SNI).
//! - [`rate_limiter`]: Configurable per-path rate limiting.
//! - [`trace`]: Request/response logging via `tracing`.
//!
//! Authentication and authorization middleware live in [`crate::client_server::auth::middleware`].

pub mod pubky_host;
pub mod rate_limiter;
pub mod trace;
