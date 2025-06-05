//! Server error
use axum::{http::StatusCode, response::IntoResponse};

use crate::persistence::files::{FileIoError, WriteStreamError};

pub(crate) type HttpResult<T, E = HttpError> = core::result::Result<T, E>;

#[derive(Debug, Clone)]
pub(crate) struct HttpError {
    // #[serde(with = "serde_status_code")]
    status: StatusCode,
    detail: Option<String>,
}

impl Default for HttpError {
    fn default() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            detail: None,
        }
    }
}

impl HttpError {
    /// Create a new [`Error`].
    pub fn new(status_code: StatusCode, message: Option<impl ToString>) -> HttpError {
        Self {
            status: status_code,
            detail: message.map(|m| m.to_string()),
        }
    }

    pub fn not_found() -> HttpError {
        Self::new(StatusCode::NOT_FOUND, Some("Not Found"))
    }

    pub fn internal_server() -> HttpError {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, Some("Internal server error"))
    }

    pub fn bad_request(message: impl ToString) -> HttpError {
        Self::new(StatusCode::BAD_REQUEST, Some(message))
    }

    pub fn insufficient_storage() -> HttpError {
        Self::new(StatusCode::INSUFFICIENT_STORAGE, Some("Disk space quota exceeded"))
    }

    pub fn forbidden() -> HttpError {
        Self::new(StatusCode::FORBIDDEN, Some("Forbidden"))
    }

    pub fn unauthorized() -> HttpError {
        Self::new(StatusCode::UNAUTHORIZED, Some("Unauthorized"))
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::response::Response {
        match self.detail {
            Some(detail) => (self.status, detail).into_response(),
            _ => (self.status,).into_response(),
        }
    }
}

// === INTERNAL_SERVER_ERROR ===
// Very common errors that we can just convert to a Internal Server Error.
// This way, we can use `?` to propagate errors without having to handle them.

impl From<std::io::Error> for HttpError {
    fn from(error: std::io::Error) -> Self {
        tracing::error!(?error);
        Self::internal_server()
    }
}


// LMDB errors
impl From<heed::Error> for HttpError {
    fn from(error: heed::Error) -> Self {
        tracing::error!(?error);
        Self::internal_server()
    }
}

// Anyhow errors
impl From<anyhow::Error> for HttpError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(?error);
        Self::internal_server()
    }
}

impl From<postcard::Error> for HttpError {
    fn from(error: postcard::Error) -> Self {
        tracing::error!(?error);
        Self::internal_server()
    }
}

impl From<axum::Error> for HttpError {
    fn from(error: axum::Error) -> Self {
        tracing::error!(?error);
        Self::internal_server()
    }
}

impl From<axum::http::Error> for HttpError {
    fn from(error: axum::http::Error) -> Self {
        tracing::error!(?error);
        Self::internal_server()
    }
}

impl From<FileIoError> for HttpError {
    fn from(error: FileIoError) -> Self {
        match error {
            FileIoError::NotFound => Self::not_found(),
            FileIoError::StreamBroken(WriteStreamError::DiskSpaceQuotaExceeded) => Self::insufficient_storage(),
            FileIoError::StreamBroken(_) => Self::bad_request("Stream broken"),
            e => {
                tracing::error!(?e);
                Self::internal_server()
            },
        }
    }
}

impl From<pubky_common::auth::Error> for HttpError {
    fn from(error: pubky_common::auth::Error) -> Self {
        Self::bad_request(error)
    }
}