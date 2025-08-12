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
/// This provides a simplified, actionable set of error categories for developers
/// to handle in their code.
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
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
/// @property data - An optional payload containing structured context, such as an HTTP status code.
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

    #[serde(
        with = "serde_wasm_bindgen::preserve",
        skip_serializing_if = "JsValue::is_undefined"
    )]
    #[tsify(optional, type = "any")]
    pub data: JsValue,
}

// --- Constructors for Ergonomics ---
impl PubkyJsError {
    /// Creates a new error with a name and message.
    pub fn new(name: PubkyErrorName, message: String) -> Self {
        Self {
            name,
            message,
            data: JsValue::UNDEFINED,
        }
    }

    /// Creates a new error with a name, message, and structured data payload.
    pub fn new_with_data(name: PubkyErrorName, message: String, data: JsValue) -> Self {
        Self {
            name,
            message,
            data,
        }
    }
}

// --- Rust-to-JavaScript pubky::Error Conversion Pipeline ---

/// Converts a native `pubky::Error` into a `PubkyJsError`.
impl From<pubky::Error> for PubkyJsError {
    fn from(err: pubky::Error) -> Self {
        let mut data = JsValue::UNDEFINED;
        let name = match &err {
            pubky::Error::Request(req_err) => {
                if let RequestError::Server { status, .. } = req_err {
                    // Manually construct the JS object for the data payload.
                    let obj = js_sys::Object::new();
                    js_sys::Reflect::set(
                        &obj,
                        &"statusCode".into(),
                        &(status.as_u16() as f64).into(),
                    )
                    .unwrap();
                    data = obj.into();
                }
                PubkyErrorName::RequestError
            }
            pubky::Error::Parse(_) => PubkyErrorName::InvalidInput,
            pubky::Error::Authentication(_) => PubkyErrorName::AuthenticationError,
            pubky::Error::Pkarr(_) => PubkyErrorName::PkarrError,
        };

        Self::new_with_data(name, err.to_string(), data)
    }
}

/// Converts a `pubky::BuildError` into a `PubkyJsError`.
impl From<BuildError> for PubkyJsError {
    fn from(err: BuildError) -> Self {
        Self::new(PubkyErrorName::InternalError, err.to_string())
    }
}

/// Converts a `pubky_common::recovery_file::Error` into a `PubkyJsError`.
impl From<RecoveryFileError> for PubkyJsError {
    fn from(err: RecoveryFileError) -> Self {
        Self::new(PubkyErrorName::ClientStateError, err.to_string())
    }
}

/// Converts a `pubky_common::capabilities::Error` into a `PubkyJsError`.
impl From<CapabilitiesError> for PubkyJsError {
    fn from(err: CapabilitiesError) -> Self {
        Self {
            name: PubkyErrorName::InvalidInput,
            message: err.to_string(),
            data: JsValue::UNDEFINED,
        }
    }
}

/// Converts a `url::ParseError` into a `PubkyJsError`.
impl From<url::ParseError> for PubkyJsError {
    fn from(err: url::ParseError) -> Self {
        Self::new(PubkyErrorName::InvalidInput, err.to_string())
    }
}

/// Converts a `pkarr::PublicKeyError` into a `PubkyJsError`.
impl From<PublicKeyError> for PubkyJsError {
    fn from(err: PublicKeyError) -> Self {
        Self::new(PubkyErrorName::InvalidInput, err.to_string())
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
