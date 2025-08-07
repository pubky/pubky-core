use crate::js_error::JsError;

/// A convenient `Result` type alias for fallible functions exposed to WebAssembly.
///
/// An `Err` variant will be automatically converted into a structured JavaScript exception
/// that can be caught on the JS side.
pub type JsResult<T> = Result<T, JsError>;
