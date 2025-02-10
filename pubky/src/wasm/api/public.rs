//! Wasm bindings for the /pub/ api

use js_sys::Array;
use wasm_bindgen::prelude::*;

use super::super::Client;

#[wasm_bindgen]
impl Client {
    /// Returns a list of Pubky urls (as strings).
    ///
    /// - `url`:     The Pubky url (string) to the directory you want to list its content.
    /// - `cursor`:  Either a full `pubky://` Url (from previous list response),
    ///                 or a path (to a file or directory) relative to the `url`
    /// - `reverse`: List in reverse order
    /// - `limit`    Limit the number of urls in the response
    /// - `shallow`: List directories and files, instead of flat list of files.
    #[wasm_bindgen]
    pub async fn list(
        &self,
        url: &str,
        cursor: Option<String>,
        reverse: Option<bool>,
        limit: Option<u16>,
        shallow: Option<bool>,
    ) -> Result<Array, JsValue> {
        // TODO: try later to return Vec<String> from async function.

        if let Some(cursor) = cursor {
            return self
                .0
                .list(url)
                .map_err(|e| JsValue::from_str(&e.to_string()))?
                .reverse(reverse.unwrap_or(false))
                .limit(limit.unwrap_or(u16::MAX))
                .cursor(&cursor)
                .shallow(shallow.unwrap_or(false))
                .send()
                .await
                .map(|urls| {
                    let js_array = Array::new();

                    for url in urls {
                        js_array.push(&JsValue::from_str(&url));
                    }

                    js_array
                })
                .map_err(|e| JsValue::from_str(&e.to_string()));
        }

        self.0
            .list(url)
            .map_err(|e| JsValue::from_str(&e.to_string()))?
            .reverse(reverse.unwrap_or(false))
            .limit(limit.unwrap_or(u16::MAX))
            .shallow(shallow.unwrap_or(false))
            .send()
            .await
            .map(|urls| {
                let js_array = Array::new();

                for url in urls {
                    js_array.push(&JsValue::from_str(&url));
                }

                js_array
            })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
