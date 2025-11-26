use std::str::FromStr;

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;

use crate::{js_error::{JsResult, PubkyError, PubkyErrorName}, wrappers::keys::PublicKey};


#[wasm_bindgen]
pub struct SignupDeepLink(pubky::deep_links::SignupDeepLink);

#[wasm_bindgen]
impl SignupDeepLink {
    #[wasm_bindgen(js_name = "parse")]
    pub fn try_from(url: &str) -> JsResult<Self> {
        Ok(Self(pubky::deep_links::SignupDeepLink::from_str(url).map_err(|e| PubkyError::new(PubkyErrorName::InvalidInput, format!("Invalid signup deep link: {}", e)))?))
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

    #[wasm_bindgen(getter)]
    pub fn homeserver(&self) -> PublicKey {
        PublicKey(self.0.homeserver().clone())
    }

    #[wasm_bindgen(js_name = "signupToken", getter)]
    pub fn signup_token(&self) -> Option<String> {
        self.0.signup_token().clone()
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}