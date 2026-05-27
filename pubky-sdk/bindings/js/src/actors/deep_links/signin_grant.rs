use std::str::FromStr;

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use crate::{
    js_error::{JsResult, PubkyError, PubkyErrorName},
    wrappers::keys::PublicKey,
};

#[wasm_bindgen]
pub struct SigninGrantDeepLink(pubky::deep_links::SigninGrantDeepLink);

#[wasm_bindgen]
impl SigninGrantDeepLink {
    #[wasm_bindgen(js_name = "parse")]
    pub fn try_from(url: &str) -> JsResult<Self> {
        Ok(Self(
            pubky::deep_links::SigninGrantDeepLink::from_str(url).map_err(|e| {
                PubkyError::new(
                    PubkyErrorName::InvalidInput,
                    format!("Invalid signin grant deep link: {}", e),
                )
            })?,
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> String {
        self.0.params().capabilities.to_string()
    }

    #[wasm_bindgen(js_name = "baseRelayUrl", getter)]
    pub fn base_relay_url(&self) -> String {
        self.0.params().relay.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn secret(&self) -> Uint8Array {
        Uint8Array::from(self.0.params().secret.as_ref())
    }

    #[wasm_bindgen(js_name = "clientId", getter)]
    pub fn client_id(&self) -> String {
        self.0.params().client_id.to_string()
    }

    #[wasm_bindgen(js_name = "clientPublicKey", getter)]
    pub fn client_public_key(&self) -> PublicKey {
        PublicKey(self.0.params().client_pk.clone())
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
