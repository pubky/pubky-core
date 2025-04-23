//! Server error

use axum::{http::StatusCode, response::IntoResponse};

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
        tracing::debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

// LMDB errors
impl From<heed::Error> for HttpError {
    fn from(error: heed::Error) -> Self {
        tracing::debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

// Anyhow errors
impl From<anyhow::Error> for HttpError {
    fn from(error: anyhow::Error) -> Self {
        tracing::debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<postcard::Error> for HttpError {
    fn from(error: postcard::Error) -> Self {
        tracing::debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<axum::Error> for HttpError {
    fn from(error: axum::Error) -> Self {
        tracing::debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}

impl From<axum::http::Error> for HttpError {
    fn from(error: axum::http::Error) -> Self {
        tracing::debug!(?error);
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error.into())
    }
}
