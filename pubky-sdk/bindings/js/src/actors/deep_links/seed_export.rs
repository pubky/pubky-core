use std::str::FromStr;

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use crate::js_error::{JsResult, PubkyError, PubkyErrorName};

#[wasm_bindgen]
pub struct SeedExportDeepLink(pubky::deep_links::SeedExportDeepLink);

#[wasm_bindgen]
impl SeedExportDeepLink {
    #[wasm_bindgen(js_name = "parse")]
    pub fn try_from(url: &str) -> JsResult<Self> {
        Ok(Self(
            pubky::deep_links::SeedExportDeepLink::from_str(url).map_err(|e| {
                PubkyError::new(
                    PubkyErrorName::InvalidInput,
                    format!("Invalid seed export deep link: {}", e),
                )
            })?,
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn secret(&self) -> Uint8Array {
        Uint8Array::from(self.0.secret().as_ref())
    }

    #[allow(
        clippy::inherent_to_string,
        reason = "Display trait doesn't work with wasm-bindgen"
    )]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}
