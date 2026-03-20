//! Data persistence layer.
//!
//! - [`sql`]: PostgreSQL storage for users, sessions, entries (file metadata),
//!   events, and signup codes. Uses the repository pattern with `sea-query`.
//! - [`files`]: Blob storage via OpenDAL (filesystem, in-memory, or GCS) with
//!   layered middleware for quota enforcement, metadata tracking, and event creation.
//! - [`lmdb`]: Legacy LMDB store, retained for backward-compatible migration to SQL.

pub mod files;
pub mod lmdb;
pub mod sql;
