
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

#[derive(Debug, thiserror::Error)]
pub enum WriteFileFromStreamError {
    #[error("LMDB error: {0}")]
    LmDB(#[from] heed::Error),
    #[error("OpenDAL error: {0}")]
    OpenDAL(#[from] opendal::Error),
    #[error("Temp file error: {0}")]
    TempFile(#[from] std::io::Error),
    #[error(transparent)]
    StreamBroken(#[from] WriteStreamError),
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}
