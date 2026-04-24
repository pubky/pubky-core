/// How often (in seconds) background cleanup tasks run to evict expired
/// rate-limiter entries and shrink internal maps.
const CLEANUP_INTERVAL_SECS: u64 = 60;

mod extract_ip;
mod layer;
mod limiter_pool;
mod request_info;
mod throttle;

pub use layer::*;
