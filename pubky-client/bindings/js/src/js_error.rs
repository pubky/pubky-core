use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use pkarr::errors::PublicKeyError;
use pubky::errors::{BuildError, Error, RequestError};
use pubky_common::capabilities::Error as CapabilitiesError;
use pubky_common::recovery_file::Error as RecoveryFileError;

// --- TypeScript Documentation & Schema ---

/// A union type of all possible machine-readable codes for the `name` property
/// of a {@link PubkyError}.
///
/// This provides a simplified, actionable set of error categories for developers
/// to handle in their code.
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub enum PubkyErrorName {
    /// A network or server request failed. Check the network connection or retry.
    RequestError,
    /// The error was caused by invalid user input, such as a malformed URL.
    InvalidInputError,
    /// An error occurred during login, signup, or session validation.
    AuthenticationError,
    /// A failure in the underlying Pkarr DHT protocol.
    PkarrError,
    /// An error related to client state, like a corrupt recovery file.
    ClientStateError,
    /// An unexpected or internal error occurred. This may indicate a bug.
    InternalError,
}

/// Represents the standard error structure for all exceptions thrown by the Pubky
/// WASM client.
///
/// @property name - A machine-readable error code from {@link PubkyErrorName}. Use this for programmatic error handling.
/// @property message - A human-readable, descriptive error message suitable for logging.
/// @property data - An optional payload containing structured context, such as an HTTP status code.
///
/// @example
/// ```typescript
/// try {
///   await client.signup(...);
/// } catch (e) {
///   const error = e as PubkyError;
///   if (error.name === 'RequestError' && error.data?.statusCode === 404) {
///     // Handle not found...
///   }
/// }
/// ```
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyError {
    pub name: PubkyErrorName,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// --- Rust-to-JavaScript Error Conversion Pipeline ---

/// Converts a native `pubky::Error` into a `PubkyError`.
impl From<Error> for PubkyError {
    fn from(err: Error) -> Self {
        let mut data = None;
        let name = match &err {
            Error::Request(req_err) => {
                if let RequestError::Server { status, .. } = req_err {
                    data = Some(serde_json::json!({ "statusCode": status.as_u16() }));
                }
                PubkyErrorName::RequestError
            }
            Error::Parse(_) => PubkyErrorName::InvalidInputError,
            Error::Authentication(_) => PubkyErrorName::AuthenticationError,
            Error::Pkarr(_) => PubkyErrorName::PkarrError,
        };

        Self {
            name,
            message: err.to_string(),
            data,
        }
    }
}

/// Converts a `pubky::BuildError` into a `PubkyError`.
impl From<BuildError> for PubkyError {
    fn from(err: BuildError) -> Self {
        Self {
            name: PubkyErrorName::InternalError,
            message: err.to_string(),
            data: None,
        }
    }
}

/// Converts a `pubky_common::recovery_file::Error` into a `PubkyError`.
impl From<RecoveryFileError> for PubkyError {
    fn from(err: RecoveryFileError) -> Self {
        Self {
            name: PubkyErrorName::ClientStateError,
            message: err.to_string(),
            data: None,
        }
    }
}

/// Converts a `pubky_common::capabilities::Error` into a `PubkyError`.
impl From<CapabilitiesError> for PubkyError {
    fn from(err: CapabilitiesError) -> Self {
        Self {
            name: PubkyErrorName::InvalidInputError,
            message: err.to_string(),
            data: None,
        }
    }
}

/// Converts a `url::ParseError` into a `PubkyError`.
impl From<url::ParseError> for PubkyError {
    fn from(err: url::ParseError) -> Self {
        Self {
            name: PubkyErrorName::InvalidInputError,
            message: err.to_string(),
            data: None,
        }
    }
}

/// Converts a `pkarr::PublicKeyError` into a `PubkyError`.
impl From<PublicKeyError> for PubkyError {
    fn from(err: PublicKeyError) -> Self {
        Self {
            name: PubkyErrorName::InvalidInputError,
            message: err.to_string(),
            data: None,
        }
    }
}

/// Converts a generic `JsValue` error into a `PubkyError`.
impl From<JsValue> for PubkyError {
    fn from(err: JsValue) -> Self {
        let message = err
            .as_string()
            .unwrap_or_else(|| "An unknown JavaScript error occurred.".to_string());
        Self {
            name: PubkyErrorName::InternalError,
            message,
            data: None,
        }
    }
}

/// Converts a simple string slice error message into a `PubkyError`.
impl From<&str> for PubkyError {
    fn from(err: &str) -> Self {
        Self {
            name: PubkyErrorName::InternalError,
            message: err.to_string(),
            data: None,
        }
    }
}
