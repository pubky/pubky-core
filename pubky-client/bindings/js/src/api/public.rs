//! Wasm bindings for the /pub/ API

use wasm_bindgen::prelude::*;

use crate::constructor::Client;
use crate::js_result::JsResult;

#[wasm_bindgen]
impl Client {
    /// Returns a list of Pubky URLs (as strings) from a directory.
    ///
    /// @param {string} url - The `pubky://` URL of the directory to list.
    /// @param {string | undefined} cursor - A cursor to paginate results, from a previous response.
    /// @param {boolean | undefined} reverse - If `true`, lists items in reverse order.
    /// @param {number | undefined} limit - The maximum number of URLs to return.
    /// @param {boolean | undefined} shallow - If `true`, lists directories as single entries instead of recursively listing files.
    /// @returns {Promise<string[]>} A promise that resolves to an array of URL strings.
    /// @throws Will throw an error if the request fails.
    #[wasm_bindgen]
    pub async fn list(
        &self,
        url: &str,
        cursor: Option<String>,
        reverse: Option<bool>,
        limit: Option<u16>,
        shallow: Option<bool>,
    ) -> JsResult<Vec<String>> {
        // Start with the basic list builder from the core client.
        let mut builder = self.inner.list(url);

        // Conditionally apply each optional parameter to the builder.
        if let Some(c) = &cursor {
            builder = builder.cursor(c);
        }
        if let Some(r) = reverse {
            builder = builder.reverse(r);
        }
        if let Some(l) = limit {
            builder = builder.limit(l);
        }
        if let Some(s) = shallow {
            builder = builder.shallow(s);
        }

        // Send the fully configured request and map the error type for JS.
        builder
            .send()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
