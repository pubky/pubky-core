
/// A unified error type for when files are written
/// to the storage and database.
#[derive(Debug, thiserror::Error)]
pub enum WriteStreamError {
    #[error("Axum error: {0}")]
    Axum(#[from] axum::Error),
    #[error("Disk space quota exceeded")]
    DiskSpaceQuotaExceeded,
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}