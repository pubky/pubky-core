use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use pkarr::errors::PublicKeyError;
use pubky::{BuildError, Error};
use pubky_common::{
    capabilities::Error as CapabilitiesError, recovery_file::Error as RecoveryFileError,
};

/// A union type of all possible machine-readable codes for the `name` property
/// of a {@link PubkyError}.
#[derive(Tsify, Serialize, Deserialize)]
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
/// @property statusCode - If the error was an HTTP error, this field contains the HTTP status code.
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
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyError {
    pub name: PubkyErrorName,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

// --- Rust-to-JavaScript Error Conversion Pipeline ---

/// An intermediate representation for converting Rust errors into a structured JS exception.
#[derive(Debug)]
pub struct JsError {
    pub name: String,
    pub message: String,
    pub status_code: Option<u16>,
}

/// Converts a native `pubky::Error` into a `JsError`.
impl From<Error> for JsError {
    fn from(err: Error) -> Self {
        let mut status_code = None;
        let name = match &err {
            Error::HttpStatus { status, .. } => {
                status_code = Some(status.as_u16());
                "RequestError"
            }
            Error::Http(_) | Error::HomeserverNotFound => "RequestError",
            Error::Url(_) => "InvalidInput",
            Error::Authentication(_) => "AuthenticationError",
            Error::Pkarr(_) => "PkarrError",
        };

        Self {
            name: name.to_string(),
            message: err.to_string(),
            status_code,
        }
    }
}

/// Converts a `pubky::BuildError` into a `JsError`.
impl From<BuildError> for JsError {
    fn from(err: BuildError) -> Self {
        Self {
            name: "InternalError".to_string(),
            message: err.to_string(),
            status_code: None,
        }
    }
}

/// Converts a `pubky_common::recovery_file::Error` into a `JsError`.
impl From<RecoveryFileError> for JsError {
    fn from(err: RecoveryFileError) -> Self {
        Self {
            name: "ClientStateError".to_string(),
            message: err.to_string(),
            status_code: None,
        }
    }
}

/// Converts a `url::ParseError` into a `JsError`.
impl From<url::ParseError> for JsError {
    fn from(err: url::ParseError) -> Self {
        Self {
            name: "InvalidInput".to_string(),
            message: err.to_string(),
            status_code: None,
        }
    }
}

/// Converts a `pkarr::PublicKeyError` into a `JsError`.
impl From<PublicKeyError> for JsError {
    fn from(err: PublicKeyError) -> Self {
        Self {
            name: "InvalidInput".to_string(),
            message: err.to_string(),
            status_code: None,
        }
    }
}

/// Converts a `pubky_common::capabilities::Error` into a `JsError`.
impl From<CapabilitiesError> for JsError {
    fn from(err: CapabilitiesError) -> Self {
        Self {
            name: "InvalidInput".to_string(),
            message: err.to_string(),
            status_code: None,
        }
    }
}

/// Converts a generic `JsValue` error into a `JsError`.
impl From<JsValue> for JsError {
    fn from(err: JsValue) -> Self {
        let message = err
            .as_string()
            .unwrap_or_else(|| "An unknown JavaScript error occurred.".to_string());
        Self {
            name: "InternalError".to_string(),
            message,
            status_code: None,
        }
    }
}

/// Converts `JsError` into a structured `JsValue` for throwing as a JavaScript exception.
impl From<JsError> for JsValue {
    fn from(err: JsError) -> Self {
        let obj = js_sys::Object::new();

        js_sys::Reflect::set(&obj, &"name".into(), &err.name.into()).unwrap();
        js_sys::Reflect::set(&obj, &"message".into(), &err.message.into()).unwrap();

        if let Some(status_code) = err.status_code {
            if let Ok(js_status_code) = serde_wasm_bindgen::to_value(&status_code) {
                js_sys::Reflect::set(&obj, &"status_code".into(), &js_status_code).unwrap();
            }
        }

        obj.into()
    }
}
