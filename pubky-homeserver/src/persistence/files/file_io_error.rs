/// Error type for file operations.
#[derive(Debug, thiserror::Error)]
pub enum FileIoError {
    #[error("File not found")]
    NotFound,
    #[error("DB error: {0}")]
    Db(#[from] heed::Error),
    #[error("DB error: {0}")]
    SqlDb(#[from] sqlx::Error),
    #[error("DB serialization error: {0}")]
    DbSerialization(#[from] postcard::Error),
    #[error("OpenDAL error: {0}")]
    OpenDAL(#[from] opendal::Error),
    #[error("Temp file error: {0}")]
    TempFile(#[from] std::io::Error),
    #[error(transparent)]
    StreamBroken(#[from] WriteStreamError),
    #[error("Disk space quota exceeded")]
    DiskSpaceQuotaExceeded,
}

/// A unified error type for writing streams.
#[derive(Debug, thiserror::Error)]
pub enum WriteStreamError {
    #[error("Axum error: {0}")]
    Axum(#[from] axum::Error),
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}
