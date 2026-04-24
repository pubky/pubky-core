//! Tower middleware layers for the client server.
//!
//! - [`authz`]: Authorization — enforces read/write permissions based on session capabilities.
//! - [`pubky_host`]: Extracts the tenant public key from the request Host header (TLS SNI).
//! - [`rate_limiter`]: Configurable per-path request rate limiting, keyed by IP or user.
//! - [`user_bandwidth_budget`]: Per-user bandwidth budgets (read/write byte quotas per time window).
//! - [`user_limit_resolver`]: Resolves per-user limits from DB (with cache) and inserts them into request extensions for usage by other layers.
//! - [`trace`]: Request/response logging via `tracing`.

pub mod authz;
pub mod pubky_host;
pub mod rate_limiter;
pub mod trace;
pub mod user_bandwidth_budget;
pub mod user_limit_resolver;
