use std::fmt::Display;

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use pkarr::errors::PublicKeyError;
use pubky::errors::{BuildError, RequestError};
use pubky_common::capabilities::Error as CapabilitiesError;
use pubky_common::recovery_file::Error as RecoveryFileError;

// --- TypeScript Documentation & Schema ---

/// A union type of all possible machine-readable codes for the `name` property
/// of a {@link PubkyJsError}.
///
/// Provides a simplified, actionable set of error categories for developers
/// to handle in their code.
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "PascalCase")]
pub enum PubkyErrorName {
    /// A network or server request failed. Check the network connection or retry.
    RequestError,
    /// The error was caused by invalid user input, such as a malformed URL.
    InvalidInput,
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
/// @property data - An optional payload containing structured context for an error. For a `RequestError`, this may contain an object with the HTTP status code, e.g., `{ statusCode: 404 }`.
///
/// @example
/// ```typescript
/// try {
///   await client.signup(...);
/// } catch (e) {
///   const error = e as PubkyJsError;
///   if (error.name === 'RequestError' && error.data?.statusCode === 404) {
///     // Handle not found...
///   }
/// }
/// ```
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyJsError {
    pub name: PubkyErrorName,
    pub message: String,

    /// For `RequestError::Server`, this carries the numeric HTTP status code (e.g. 404).
    /// Otherwise `undefined` on the JS side.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[tsify(optional)]
    pub status_code: Option<u16>,
}

// --- Constructors for Ergonomics ---
impl PubkyJsError {
    /// Creates a new error with a name and message.
    pub fn new<T: Display>(name: PubkyErrorName, message: T) -> Self {
        Self {
            name,
            message: message.to_string(),
            status_code: None,
        }
    }

    /// Creates a new error with a name, message, and structured data payload.
    pub fn new_with_status<T: Display>(name: PubkyErrorName, message: T, status: u16) -> Self {
        Self {
            name,
            message: message.to_string(),
            status_code: Some(status),
        }
    }
}

// --- Rust-to-JavaScript pubky::Error Conversion Pipeline ---

/// Converts a native `pubky::Error` into a `PubkyJsError`.
impl From<pubky::Error> for PubkyJsError {
    fn from(err: pubky::Error) -> Self {
        let name = match &err {
            pubky::Error::Request(_) => PubkyErrorName::RequestError,
            pubky::Error::Parse(_) => PubkyErrorName::InvalidInput,
            pubky::Error::Authentication(_) => PubkyErrorName::AuthenticationError,
            pubky::Error::Pkarr(_) => PubkyErrorName::PkarrError,
            pubky::Error::Build(_) => PubkyErrorName::InternalError,
        };

        // If this was a server error, attach status_code; else leave it None.
        if let pubky::Error::Request(RequestError::Server { status, .. }) = &err {
            return Self::new_with_status(name, &err, status.as_u16());
        }
        Self::new(name, err)
    }
}

/// Converts a `pubky::BuildError` into a `PubkyJsError`.
impl From<BuildError> for PubkyJsError {
    fn from(err: BuildError) -> Self {
        Self::new(PubkyErrorName::InternalError, err)
    }
}

/// Converts a `pubky_common::recovery_file::Error` into a `PubkyJsError`.
impl From<RecoveryFileError> for PubkyJsError {
    fn from(err: RecoveryFileError) -> Self {
        Self::new(PubkyErrorName::ClientStateError, err)
    }
}

/// Converts a `pubky_common::capabilities::Error` into a `PubkyJsError`.
impl From<CapabilitiesError> for PubkyJsError {
    fn from(err: CapabilitiesError) -> Self {
        Self::new(PubkyErrorName::InvalidInput, err.to_string())
    }
}

/// Converts a `url::ParseError` into a `PubkyJsError`.
impl From<url::ParseError> for PubkyJsError {
    fn from(err: url::ParseError) -> Self {
        Self::new(PubkyErrorName::InvalidInput, err)
    }
}

/// Converts a `pkarr::PublicKeyError` into a `PubkyJsError`.
impl From<PublicKeyError> for PubkyJsError {
    fn from(err: PublicKeyError) -> Self {
        Self::new(PubkyErrorName::InvalidInput, err)
    }
}

/// Converts a `serde_wasm_bindgen::Error` (JS <-> Rust value (de)serialization) into `PubkyJsError`.
impl From<serde_wasm_bindgen::Error> for PubkyJsError {
    fn from(err: serde_wasm_bindgen::Error) -> Self {
        // Treat bad JS values / schema mismatches as invalid input.
        PubkyJsError::new(PubkyErrorName::InvalidInput, err)
    }
}

/// Converts a `reqwest::Error` into a `PubkyJsError`.
impl From<reqwest::Error> for PubkyJsError {
    fn from(err: reqwest::Error) -> Self {
        // Try to propagate status when present
        if let Some(status) = err.status() {
            PubkyJsError::new_with_status(
                PubkyErrorName::RequestError,
                err.to_string(),
                status.as_u16(),
            )
        } else {
            PubkyJsError::new(PubkyErrorName::RequestError, err.to_string())
        }
    }
}

/// Converts a generic `JsValue` error into a `PubkyJsError`.
impl From<JsValue> for PubkyJsError {
    fn from(err: JsValue) -> Self {
        let message = err
            .as_string()
            .unwrap_or_else(|| "An unknown JavaScript error occurred.".to_string());
        Self::new(PubkyErrorName::InternalError, message)
    }
}
