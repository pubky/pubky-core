use std::time::Duration;

pub const DEFAULT_USER_AGENT: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"),);
pub const DEFAULT_RELAYS: &[&str] = &["https://pkarr.pubky.org/", "https://pkarr.pubky.app/"];
pub const DEFAULT_MAX_RECORD_AGE: Duration = Duration::from_secs(60 * 60);
