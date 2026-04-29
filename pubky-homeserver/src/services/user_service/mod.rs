//! User service — coordinates user lookups, creation, quota enforcement, and caching.

mod quota_cache;
mod service;

pub use service::{UserService, FILE_METADATA_SIZE};
