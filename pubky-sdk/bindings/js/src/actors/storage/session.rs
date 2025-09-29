// js/src/client/storage/session.rs
use js_sys::Uint8Array;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::public::ResourceStats;
use crate::js_result::JsResult;

#[wasm_bindgen]
pub struct SessionStorage(pub(crate) pubky::SessionStorage);

#[wasm_bindgen]
impl SessionStorage {
    /// Directory listing in session scope (absolute paths only).
    #[wasm_bindgen]
    pub async fn list(
        &self,
        path: &str,
        cursor: Option<String>,
        reverse: Option<bool>,
        limit: Option<u16>,
        shallow: Option<bool>,
    ) -> JsResult<Vec<String>> {
        let mut b = self.0.list(path)?;
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

    /// GET as bytes.
    #[wasm_bindgen(js_name = "getBytes")]
    pub async fn get_bytes(&self, path: &str) -> JsResult<Uint8Array> {
        let resp = self.0.get(path).await?;
        let bytes = resp.bytes().await?;
        Ok(Uint8Array::from(bytes.as_ref()))
    }

    /// GET as UTF-8 text.
    #[wasm_bindgen(js_name = "getText")]
    pub async fn get_text(&self, path: &str) -> JsResult<String> {
        let resp = self.0.get(path).await?;
        Ok(resp.text().await?)
    }

    /// GET JSON (sets Accept: application/json)
    #[wasm_bindgen(js_name = "getJson")]
    pub async fn get_json(&self, addr: &str) -> JsResult<JsValue> {
        let v: serde_json::Value = self.0.get_json(addr).await?;
        let ser = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        Ok(v.serialize(&ser)?)
    }

    /// HEAD existence check.
    #[wasm_bindgen]
    pub async fn exists(&self, path: &str) -> JsResult<bool> {
        Ok(self.0.exists(path).await?)
    }

    /// HEAD stats.
    #[wasm_bindgen]
    pub async fn stats(&self, path: &str) -> JsResult<Option<ResourceStats>> {
        Ok(self.0.stats(path).await?.map(Into::into))
    }

    /// PUT bytes.
    #[wasm_bindgen(js_name = "putBytes")]
    pub async fn put_bytes(&self, path: &str, body: &[u8]) -> JsResult<()> {
        self.0.put(path, body.to_vec()).await?;
        Ok(())
    }

    /// PUT UTF-8 text.
    #[wasm_bindgen(js_name = "putText")]
    pub async fn put_text(&self, path: &str, body: &str) -> JsResult<()> {
        self.0.put(path, body.as_bytes().to_vec()).await?;
        Ok(())
    }

    /// PUT JSON (sets Content-Type: application/json)
    #[wasm_bindgen(js_name = "putJson")]
    pub async fn put_json(&self, path: &str, body: JsValue) -> JsResult<()> {
        let v: serde_json::Value = serde_wasm_bindgen::from_value(body)?;
        self.0.put_json(path, &v).await?;
        Ok(())
    }

    /// DELETE.
    #[wasm_bindgen]
    pub async fn delete(&self, path: &str) -> JsResult<()> {
        self.0.delete(path).await?;
        Ok(())
    }
}
