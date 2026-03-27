//! Request middleware for the client server.
//!
//! Each module colocates its Tower layer with the types it injects into request
//! extensions (extractors), keeping producer and consumer together.
//!
//! - [`authentication`]: Resolves credentials into an [`AuthSession`] identity.
//! - [`authorization`]: [`WriteAccess`] extractor for write capability checks.
//! - [`pubky_host`]: Extracts the tenant public key from the request Host header (TLS SNI).
//! - [`rate_limiter`]: Configurable per-path rate limiting.
//! - [`trace`]: Request/response logging via `tracing`.

pub mod authentication;
pub mod authorization;
pub mod pubky_host;
pub mod rate_limiter;
pub mod trace;
