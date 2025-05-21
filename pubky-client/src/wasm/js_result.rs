use wasm_bindgen::JsValue;

/// Convenient type for functions that return a Err(JsValue).
///
/// fn method() -> JsResult<T> {
///     Ok(())
/// }
///
/// fn method() -> JsResult<T> {
///     Err(JsValue::from_str("error"))
/// }
///
pub type JsResult<T> = Result<T, JsValue>;
