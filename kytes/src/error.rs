//! Main Crate Error

#[derive(thiserror::Error, Debug)]
/// Kytes crate error enum.
pub enum Error {
    /// For starter, to remove as code matures.
    #[error("Generic error: {0}")]
    Generic(String),
}
