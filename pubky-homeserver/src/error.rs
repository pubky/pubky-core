//! Server error

use axum::{
    extract::rejection::{ExtensionRejection, PathRejection, QueryRejection},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::debug;

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Debug, Clone)]
pub struct Error {
    // #[serde(with = "serde_status_code")]
    status: StatusCode,
    detail: Option<String>,
}

impl Default for Error {
    fn default() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            detail: None,
        }
    }
}

impl Error {
    pub fn with_status(status: StatusCode) -> Error {
        Self {
            status,
            detail: None,
        }
    }

    /// Create a new [`Error`].
    pub fn new(status_code: StatusCode, message: Option<impl ToString>) -> Error {
        Self {
            status: status_code,
            detail: message.map(|m| m.to_string()),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self.detail {
            Some(detail) => (self.status, detail).into_response(),
            _ => (self.status,).into_response(),
        }
    }
}

impl From<QueryRejection> for Error {
    fn from(error: QueryRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.into())
    }
}

impl From<ExtensionRejection> for Error {
    fn from(error: ExtensionRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.into())
    }
}

impl From<PathRejection> for Error {
    fn from(error: PathRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error.into())
    }
}

// === Pubky specific errors ===

impl From<pubky_common::auth::Error> for Error {
    fn from(error: pubky_common::auth::Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(error))
    }
}

impl From<pkarr::Error> for Error {
    fn from(error: pkarr::Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(error))
    }
}

// === INTERNAL_SERVER_ERROR ===

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<heed::Error> for Error {
    fn from(error: heed::Error) -> Self {
        debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<postcard::Error> for Error {
    fn from(error: postcard::Error) -> Self {
        debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<axum::Error> for Error {
    fn from(error: axum::Error) -> Self {
        debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl<T> From<flume::SendError<T>> for Error {
    fn from(error: flume::SendError<T>) -> Self {
        debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}
