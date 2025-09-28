// A builder pattern like Capabilities::build() is unusual an overkill for JS
// but we still want parsing/validation and failing fast from wrong capabilities
// strings

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::{
    js_error::{PubkyErrorName, PubkyJsError},
    js_result::JsResult,
};
use pubky_common::capabilities::{Capabilities, Capability};

/// Internal helper: normalizes capabilities and collects invalid tokens.
fn normalize_and_collect(input: &str) -> (String, Vec<String>) {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();

    for tok in input.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match Capability::try_from(tok) {
            Ok(cap) => valid.push(cap),
            Err(_) => invalid.push(tok.to_string()),
        }
    }

    let normalized = Capabilities(valid).to_string(); // normalizes action order (:rw)
    (normalized, invalid)
}

/// Validate and normalize a capabilities string.
///
/// - On success: returns a **normalized** string (e.g. `"/pub/app/:rw,/priv/foo/:r"`).
/// - On failure: throws `PubkyJsError { name: "InvalidInput", message: string }`
/// - Empty input is allowed (means “no scopes”).
#[wasm_bindgen(js_name = "validateCapabilities")]
pub fn validate_capabilities(input: &str) -> JsResult<String> {
    let (normalized, invalid) = normalize_and_collect(input);

    if !invalid.is_empty() {
        // Human-friendly message: comma-separated list of bad entries
        let joined = invalid.join(", ");

        // Structured payload for programmatic handling
        let arr = Array::new();
        for s in invalid {
            arr.push(&JsValue::from_str(&s));
        }

        return Err(PubkyJsError::new(
            PubkyErrorName::InvalidInput,
            format!("Invalid capability entries: {joined}"),
        ));
    }

    Ok(normalized)
}

/// Internal: same as `validateCapabilities` but returns a Rust error.
pub(crate) fn validate_caps_for_start(input: &str) -> Result<String, PubkyJsError> {
    validate_capabilities(input)
}
