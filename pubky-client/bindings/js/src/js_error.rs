use pkarr::errors::PublicKeyError;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// An intermediate representation for converting Rust errors into a structured JS exception.
#[derive(Debug)]
pub struct JsError {
    pub name: String,
    pub message: String,
}

/// Converts a native `pubky::Error` into a `JsError`.
impl From<pubky::Error> for JsError {
    fn from(err: pubky::Error) -> Self {
        let name = match err {
            pubky::Error::HttpStatus { .. } => "HttpStatus",
            pubky::Error::Http(_) => "HttpError",
            pubky::Error::Url(_) => "UrlError",
            pubky::Error::Pkarr(_) => "PkarrError",
            pubky::Error::InvalidUrlStructure(_) => "InvalidUrlStructure",
            pubky::Error::Session(_) => "SessionError",
            pubky::Error::VerificationFailed(_) => "VerificationFailed",
            pubky::Error::Crypto(_) => "CryptoError",
            pubky::Error::Auth(_) => "AuthError",
            pubky::Error::AuthRequestExpired => "AuthRequestExpired",
        };

        Self {
            name: name.to_string(),
            message: err.to_string(),
        }
    }
}

/// Converts a `pubky::BuildError` into a `JsError`.
impl From<pubky::BuildError> for JsError {
    fn from(err: pubky::BuildError) -> Self {
        Self {
            name: "BuildError".to_string(),
            message: err.to_string(),
        }
    }
}

/// Converts a `pubky_common::recovery_file::Error` into a `JsError`.
impl From<pubky_common::recovery_file::Error> for JsError {
    fn from(err: pubky_common::recovery_file::Error) -> Self {
        Self {
            name: "RecoveryFileError".to_string(),
            message: err.to_string(),
        }
    }
}

/// Converts a `url::ParseError` into a `JsError`.
impl From<url::ParseError> for JsError {
    fn from(err: url::ParseError) -> Self {
        Self {
            name: "UrlParseError".to_string(),
            message: err.to_string(),
        }
    }
}

/// Converts a generic `JsValue` error into a `JsError`.
/// This is a catch-all for errors originating from JavaScript APIs.
impl From<JsValue> for JsError {
    fn from(err: JsValue) -> Self {
        // Try to get a structured message from the JsValue
        let message = err
            .as_string()
            .unwrap_or_else(|| "An unknown JavaScript error occurred.".to_string());

        Self {
            name: "JavaScriptError".to_string(),
            message,
        }
    }
}

/// Converts a `pkarr::PublicKeyError` into a `JsError`.
impl From<PublicKeyError> for JsError {
    fn from(err: PublicKeyError) -> Self {
        Self {
            name: "PublicKeyError".to_string(),
            message: err.to_string(),
        }
    }
}

/// Converts a simple string slice error message into a `JsError`.
impl From<&str> for JsError {
    fn from(err: &str) -> Self {
        Self {
            name: "Error".to_string(),
            message: err.to_string(),
        }
    }
}

/// Converts `JsError` into a structured `JsValue` for throwing as a JavaScript exception.
impl From<JsError> for JsValue {
    fn from(err: JsError) -> Self {
        let obj = js_sys::Object::new();

        js_sys::Reflect::set(&obj, &"name".into(), &err.name.into()).unwrap();
        js_sys::Reflect::set(&obj, &"message".into(), &err.message.into()).unwrap();

        obj.into()
    }
}

// --- TS documentation ---

/// A union type of all possible machine-readable codes for the `name` property
/// of a {@link PubkyError}.
///
/// This provides type safety and autocompletion when checking for specific
/// error types in a `catch` block.
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub enum PubkyErrorName {
    // Variants from `impl From<PubkyError>`
    HttpStatus,
    HttpError,
    UrlError,
    PkarrError,
    InvalidUrlStructure,
    SessionError,
    VerificationFailed,
    CryptoError,
    AuthError,
    AuthRequestExpired,

    // Variant from `impl From<BuildError>`
    BuildError,

    // Variant from `impl From<RecoveryFileError>`
    RecoveryFileError,

    // Variant from `impl From<url::ParseError>`
    UrlParseError,

    // Variant from `impl From<JsValue>`
    JavaScriptError,

    // Variant from `impl From<PublicKeyError>`
    PublicKeyError,

    // Variant from `impl From<&str>`
    Error,
}

/// Represents the standard error structure for all exceptions thrown by the Pubky
/// WASM client. Functions that can fail will throw an object conforming to this
/// interface.
///
/// @property name - A machine-readable error code from {@link PubkyErrorName}. Use this for programmatic error handling, like in a `switch` or `if` statement.
/// @property message - A human-readable, descriptive error message suitable for logging or displaying to a user.
///
/// @example
/// ```typescript
/// import { Client, PubkyError } from './pubky';
///
/// const client = new Client();
///
/// try {
///   const session = await client.signup(...);
/// } catch (e) {
///   const error = e as PubkyError;
///   console.error(`Error Type: ${error.name}`);
///   console.error(`Details: ${error.message}`);
///
///   if (error.name === 'httpStatus') {
///     // Handle a specific HTTP error from the server...
///   }
/// }
/// ```
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyError {
    pub name: PubkyErrorName,
    pub message: String,
}
