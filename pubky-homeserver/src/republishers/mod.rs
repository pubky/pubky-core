mod key_republisher;
mod user_keys_republisher;

pub(crate) use key_republisher::HomeserverKeyRepublisher;
pub(crate) use user_keys_republisher::UserKeysRepublisher;

/// Errors that can occur when building a `Republishers`.
#[derive(Debug, thiserror::Error)]
pub enum KeyRepublisherBuildError {
    /// Failed to run the key republisher.
    #[error("Key republisher error: {0}")]
    KeyRepublisher(anyhow::Error),
}
