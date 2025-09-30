// js/src/client/storage/public.rs
use js_sys::Uint8Array;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::js_error::JsResult;

/// TS-friendly stats object
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStats {
    #[tsify(optional)]
    pub content_length: Option<u64>,
    #[tsify(optional)]
    pub content_type: Option<String>,
    /// Unix millis since epoch
    #[tsify(optional)]
    pub last_modified_ms: Option<u64>,
    #[tsify(optional)]
    pub etag: Option<String>,
}

impl From<pubky::ResourceStats> for ResourceStats {
    fn from(s: pubky::ResourceStats) -> Self {
        let last_modified_ms = s.last_modified.map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        });
        Self {
            content_length: s.content_length,
            content_type: s.content_type,
            last_modified_ms,
            etag: s.etag,
        }
    }
}

/// Read-only public storage using addressed paths (`"<user-z32>/pub/...")`.
#[wasm_bindgen]
pub struct PublicStorage(pub(crate) pubky::PublicStorage);

#[wasm_bindgen]
impl PublicStorage {
    /// Construct PublicStorage using global client (mainline relays).
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsResult<PublicStorage> {
        Ok(PublicStorage(pubky::PublicStorage::new()?))
    }

    /// List a directory. Results are `pubky://…` absolute URLs.
    ///
    /// @param {string} dirAddr Addressed directory (must end with `/`).
    /// @param {string|null=} cursor Optional suffix or full URL to start **after**.
    /// @param {boolean=} reverse Default `false`. When `true`, newest/lexicographically-last first.
    /// @param {number=} limit Optional result limit.
    /// @param {boolean=} shallow Default `false`. When `true`, lists only first-level entries.
    /// @returns {Promise<string[]>}
    #[wasm_bindgen]
    pub async fn list(
        &self,
        address: &str,
        cursor: Option<String>,
        reverse: Option<bool>,
        limit: Option<u16>,
        shallow: Option<bool>,
    ) -> JsResult<Vec<String>> {
        let mut b = self.0.list(address)?;
        if let Some(c) = cursor {
            b = b.cursor(&c);
        }
        if let Some(r) = reverse {
            b = b.reverse(r);
        }
        if let Some(l) = limit {
            b = b.limit(l);
        }
        if let Some(s) = shallow {
            b = b.shallow(s);
        }

        let urls = b.send().await?.into_iter().map(|u| u.to_string()).collect();
        Ok(urls)
    }

    /// Fetch bytes from an addressed path.
    ///
    /// @param {string} addr
    /// @returns {Promise<Uint8Array>}
    #[wasm_bindgen(js_name = "getBytes")]
    pub async fn get_bytes(&self, address: &str) -> JsResult<Uint8Array> {
        let resp = self.0.get(address).await?;
        let bytes = resp.bytes().await?;
        Ok(Uint8Array::from(bytes.as_ref()))
    }

    /// Fetch text from an addressed path as UTF-8 text.
    ///
    /// @param {string} addr
    /// @returns {Promise<string>}
    #[wasm_bindgen(js_name = "getText")]
    pub async fn get_text(&self, address: &str) -> JsResult<String> {
        let resp = self.0.get(address).await?;
        Ok(resp.text().await?)
    }

    /// Fetch JSON from an addressed path.
    ///
    /// @param {string} addr `"<user-z32>/pub/.../file.json"` or `pubky://<user>/pub/...`.
    /// @returns {Promise<any>}
    #[wasm_bindgen(js_name = "getJson")]
    pub async fn get_json(&self, addr: &str) -> JsResult<JsValue> {
        let v: serde_json::Value = self.0.get_json(addr).await?;
        let ser = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        Ok(v.serialize(&ser)?)
    }

    /// Check if a path exists.
    ///
    /// @param {string} addr
    /// @returns {Promise<boolean>}
    #[wasm_bindgen]
    pub async fn exists(&self, address: &str) -> JsResult<bool> {
        Ok(self.0.exists(address).await?)
    }

    /// Get metadata for a path (or `null` if missing).
    ///
    /// @param {string} addr
    /// @returns {Promise<null | ResourceStats >}
    #[wasm_bindgen]
    pub async fn stats(&self, address: &str) -> JsResult<Option<ResourceStats>> {
        Ok(self.0.stats(address).await?.map(Into::into))
    }
}
