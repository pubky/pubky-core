//! Main Crate Error

#[derive(thiserror::Error, Debug)]
/// Kytz crate error enum.
pub enum Error {
    /// For starter, to remove as code matures.
    #[error("Generic error: {0}")]
    Generic(String),

    #[error(transparent)]
    /// Error from `std::io::Error`.
    Io(#[from] std::io::Error),
}
