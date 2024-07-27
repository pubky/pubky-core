//! Main Crate Error

use pkarr::dns::SimpleDnsError;

// Alias Result to be the crate Result.
pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(thiserror::Error, Debug)]
/// Pk common Error
pub enum Error {
    /// For starter, to remove as code matures.
    #[error("Generic error: {0}")]
    Generic(String),

    #[error("Not signed in")]
    NotSignedIn,

    // === Transparent ===
    #[error(transparent)]
    Dns(#[from] SimpleDnsError),

    #[error(transparent)]
    Pkarr(#[from] pkarr::Error),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    #[cfg(not(target_arch = "wasm32"))]
    Flume(#[from] flume::RecvError),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    #[cfg(not(target_arch = "wasm32"))]
    Session(#[from] pubky_common::session::Error),
}
