//! Tower middleware layers for the client server.
//!
//! - [`authz`]: Authorization — enforces read/write permissions based on session capabilities.
//! - [`pubky_host`]: Extracts the tenant public key from the request Host header (TLS SNI).
//! - [`rate_limiter`]: Configurable per-path rate limiting.
//! - [`trace`]: Request/response logging via `tracing`.

pub mod authz;
pub mod pubky_host;
pub mod rate_limiter;
pub mod trace;
pub mod user_limit_resolver;
pub mod user_rate_limiter;
