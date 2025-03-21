//! Configuration for the server

pub const DEFAULT_REPUBLISHER_INTERVAL: u64 = 4 * 60 * 60; // 4 hours in seconds

// === Core ==
pub const DEFAULT_STORAGE_DIR: &str = "pubky";
pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;

// === IO ===
pub const DEFAULT_HTTP_PORT: u16 = 6286;
pub const DEFAULT_HTTPS_PORT: u16 = 6287;
