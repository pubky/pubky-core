//! Wasm bindings for making generic HTTP requests via a fetch-like API.

use crate::constructor::Client;
use crate::js_result::JsResult;
use js_sys::{Object, Uint8Array};
use pubky::http_client::{HttpClient, HttpResponse};
use reqwest::{
    Method, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// A response object returned by the fetch method.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct PubkyResponse {
    status: u16,
    headers: Object,
    body: Uint8Array,
}

#[wasm_bindgen]
impl PubkyResponse {
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> u16 {
        self.status
    }
    #[wasm_bindgen(getter)]
    pub fn headers(&self) -> Object {
        self.headers.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn body(&self) -> Uint8Array {
        self.body.clone()
    }
}

impl TryFrom<HttpResponse> for PubkyResponse {
    type Error = JsValue;

    fn try_from(core_response: HttpResponse) -> Result<Self, Self::Error> {
        let headers = Object::new();
        for (name, value) in core_response.headers.iter() {
            if let Ok(val_str) = value.to_str() {
                js_sys::Reflect::set(
                    &headers,
                    &JsValue::from_str(name.as_str()),
                    &JsValue::from_str(val_str),
                )?;
            }
        }

        Ok(PubkyResponse {
            status: core_response.status.as_u16(),
            headers,
            body: Uint8Array::from(&core_response.body[..]),
        })
    }
}

/// A simplified version of the web `RequestInit` object, used for the `fetch` method.
#[derive(Tsify, Serialize, Deserialize, Debug, Default)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct FetchInit {
    #[tsify(optional)]
    pub method: Option<String>,
    #[tsify(optional)]
    pub headers: Option<HashMap<String, String>>,
    #[tsify(optional)]
    pub body: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl Client {
    /// Performs an HTTP request, similar to the web `fetch` API.
    #[wasm_bindgen]
    pub async fn fetch(&self, url_str: &str, init: Option<FetchInit>) -> JsResult<PubkyResponse> {
        let init = init.unwrap_or_default();

        let method = init
            .method
            .and_then(|m| m.parse::<Method>().ok())
            .unwrap_or(Method::GET);

        let mut headers = HeaderMap::new();
        if let Some(js_headers) = init.headers {
            for (key, value) in js_headers {
                if let (Ok(h_name), Ok(h_value)) =
                    (key.parse::<HeaderName>(), value.parse::<HeaderValue>())
                {
                    headers.append(h_name, h_value);
                }
            }
        }

        // Re-implement URL rewriting here because we are bypassing the high-level `request`.
        let mut url = Url::parse(url_str).map_err(|e| JsValue::from_str(&e.to_string()))?;
        if url.scheme() == "pubky" {
            if let Some(host_and_path) = url_str.strip_prefix("pubky://") {
                let rewritten_url = format!("https://_pubky.{}", host_and_path);
                url = Url::parse(&rewritten_url).map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
        }

        // Call the low-level `http.request` to pass custom headers.
        let core_response = self
            .inner
            .http
            .request(method, url, init.body, Some(headers))
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        core_response.try_into()
    }
}
