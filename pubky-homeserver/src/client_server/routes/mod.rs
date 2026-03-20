//! HTTP route handlers for the client server.
//!
//! - [`auth`]: Signup and signin flows (session creation via AuthToken verification).
//! - [`events`]: Historical event feed and live SSE stream for file change notifications.
//! - [`root`]: Server info endpoint.
//! - [`signup_tokens`]: Signup token validation.
//! - [`tenants`]: Per-user data routes (read, write, session management).

pub(crate) mod auth;
pub(crate) mod events;
pub(crate) mod root;
pub(crate) mod signup_tokens;
pub(crate) mod tenants;
