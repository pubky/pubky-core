use wasm_bindgen::prelude::*;

use js_sys::{Array, Uint8Array};

use crate::PubkyClient;

#[wasm_bindgen]
impl PubkyClient {
    #[wasm_bindgen]
    /// Upload a small payload to a given path.
    pub async fn put(&self, url: &str, content: &[u8]) -> Result<(), JsValue> {
        self.inner_put(url, content).await.map_err(|e| e.into())
    }

    /// Download a small payload from a given path relative to a pubky author.
    #[wasm_bindgen]
    pub async fn get(&self, url: &str) -> Result<Option<Uint8Array>, JsValue> {
        self.inner_get(url)
            .await
            .map(|b| b.map(|b| (&*b).into()))
            .map_err(|e| e.into())
    }

    /// Delete a file at a path relative to a pubky author.
    #[wasm_bindgen]
    pub async fn delete(&self, url: &str) -> Result<(), JsValue> {
        self.inner_delete(url).await.map_err(|e| e.into())
    }

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
                .inner_list(url)?
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
                .map_err(|e| e.into());
        }

        self.inner_list(url)?
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
            .map_err(|e| e.into())
    }
}
