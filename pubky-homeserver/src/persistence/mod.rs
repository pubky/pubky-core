//! Data persistence layer.
//!
//! - [`sql`]: PostgreSQL storage for users, sessions, entries (file metadata),
//!   events, and signup codes. Uses the repository pattern with `sea-query`.
//! - [`files`]: Blob storage via OpenDAL (filesystem, in-memory, or GCS) with
//!   layered middleware for quota enforcement, metadata tracking, and event creation.

pub mod files;
pub mod sql;
