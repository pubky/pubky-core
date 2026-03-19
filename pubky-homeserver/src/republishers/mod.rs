//! Background DHT republishers.
//!
//! - [`HomeserverKeyRepublisher`]: Publishes the server's pkarr to the Mainline
//!   DHT every hour.
//! - [`UserKeysRepublisher`]: Periodically republishes all users' public keys
//!   to the DHT so they remain discoverable (configurable interval, minimum 30 min).

mod key_republisher;
// Allow dead code to preserve the full API surface for potential future extraction
// back into a standalone crate.
#[allow(dead_code)]
pub(crate) mod pkarr_republisher;
mod user_keys_republisher;

pub(crate) use key_republisher::HomeserverKeyRepublisher;
pub use key_republisher::KeyRepublisherBuildError;
pub(crate) use user_keys_republisher::UserKeysRepublisher;
