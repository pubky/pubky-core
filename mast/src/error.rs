//! Main Crate Error

#[derive(thiserror::Error, Debug)]
/// Mainline crate error enum.
pub enum Error {
    /// For starter, to remove as code matures.
    #[error("Generic error: {0}")]
    Generic(String),
    /// For starter, to remove as code matures.
    #[error("Static error: {0}")]
    Static(&'static str),

    #[error(transparent)]
    /// Transparent [std::io::Error]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    /// Transparent [redb::CommitError]
    CommitError(#[from] redb::CommitError),

    #[error(transparent)]
    /// Error from `redb::TransactionError`.
    TransactionError(#[from] redb::TransactionError),
}
