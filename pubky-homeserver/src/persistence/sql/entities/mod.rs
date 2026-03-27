//! Database entity definitions and repositories.
//!
//! Each submodule defines a domain entity struct and a repository with
//! query/mutation methods:
//! - [`user`]: User accounts keyed by Ed25519 public key, with quota tracking.
//! - [`session`]: Authentication sessions with capability-scoped access.
//! - [`entry`]: File metadata (path, content hash, MIME type, timestamps).
//! - [`signup_code`]: Token-gated registration codes.

pub mod entry;
pub mod grant;
pub mod grant_session;
pub mod pop_nonce;
pub mod session;
pub mod signup_code;
pub mod user;
