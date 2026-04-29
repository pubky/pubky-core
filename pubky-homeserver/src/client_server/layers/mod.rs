//! Tower middleware layers for the client server.
//!
//! - [`authz`]: Authorization — enforces read/write permissions based on session capabilities.
//! - [`pubky_host`]: Extracts the tenant public key from the request Host header (TLS SNI).
//! - [`rate_limiter`]: Configurable per-path request rate limiting, keyed by IP or user,
//!   with optional per-user speed overrides resolved from DB.
//! - [`trace`]: Request/response logging via `tracing`.

pub mod authz;
pub mod pubky_host;
pub mod rate_limiter;
pub mod trace;
