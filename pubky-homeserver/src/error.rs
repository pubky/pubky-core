//! Server error

use axum::{
    extract::rejection::{ExtensionRejection, PathRejection, QueryRejection},
    http::StatusCode,
    response::IntoResponse,
};
use pubky_common::auth::AuthnSignatureError;

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
    fn from(value: QueryRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(value))
    }
}

impl From<ExtensionRejection> for Error {
    fn from(value: ExtensionRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(value))
    }
}

impl From<PathRejection> for Error {
    fn from(value: PathRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(value))
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, Some(value))
    }
}

// === Pubky specific errors ===

impl From<AuthnSignatureError> for Error {
    fn from(value: AuthnSignatureError) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(value))
    }
}

impl From<pkarr::Error> for Error {
    fn from(value: pkarr::Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, Some(value))
    }
}
