//! Wasm bindings for making generic HTTP requests.

use crate::constructor::Client;
use crate::js_result::JsResult;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl Client {
    /// Performs a GET request.
    ///
    /// This method automatically handles `pubky://` URLs and resolves Pkarr domains.
    /// @param {string} url - The URL to fetch.
    /// @returns {Promise<Uint8Array>} A promise that resolves to the response body as a byte array.
    /// @throws Will throw an error if the request fails.
    #[wasm_bindgen]
    pub async fn get(&self, url: &str) -> JsResult<Uint8Array> {
        let response_bytes = self
            .inner
            .get(url)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Convert the Rust Vec<u8> into a JS Uint8Array.
        Ok(Uint8Array::from(&response_bytes[..]))
    }

    /// Performs a POST request with a body.
    ///
    /// This method automatically handles `pubky://` URLs and resolves Pkarr domains.
    /// @param {string} url - The URL to post to.
    /// @param {Uint8Array} body - The request body to send.
    /// @returns {Promise<Uint8Array>} A promise that resolves to the response body as a byte array.
    /// @throws Will throw an error if the request fails.
    #[wasm_bindgen]
    pub async fn post(&self, url: &str, body: &[u8]) -> JsResult<Uint8Array> {
        let response_bytes = self
            .inner
            .post(url, body.to_vec())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(Uint8Array::from(&response_bytes[..]))
    }

    // Note: You can add similar simple wrappers for `put`, `patch`, and `delete`
    // following the same pattern as `post`.
}
