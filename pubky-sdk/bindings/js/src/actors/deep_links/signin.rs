use std::str::FromStr;

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use crate::js_error::{JsResult, PubkyError, PubkyErrorName};


#[wasm_bindgen]
pub struct SigninDeepLink(pubky::deep_links::SigninDeepLink);

#[wasm_bindgen]
impl SigninDeepLink {
    #[wasm_bindgen(js_name = "parse")]
    pub fn try_from(url: &str) -> JsResult<Self> {
        Ok(Self(pubky::deep_links::SigninDeepLink::from_str(url).map_err(|e| PubkyError::new(PubkyErrorName::InvalidInput, format!("Invalid signin deep link: {}", e)))?))
    }

    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> String {
        self.0.capabilities().to_string()
    }

    #[wasm_bindgen(js_name = "baseRelayUrl", getter)]
    pub fn base_relay_url(&self) -> String {
        self.0.relay().to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn secret(&self) -> Uint8Array {
        Uint8Array::from(self.0.secret().as_ref())
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}