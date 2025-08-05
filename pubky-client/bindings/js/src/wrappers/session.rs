use pubky_common::session;

use wasm_bindgen::prelude::*;

use super::keys::PublicKey;

#[wasm_bindgen]
pub struct Session(pub(crate) session::Session);

#[wasm_bindgen]
impl Session {
    /// Return the [PublicKey] of this session
    #[wasm_bindgen]
    pub fn pubky(&self) -> PublicKey {
        self.0.pubky().clone().into()
    }

    /// Return the capabilities that this session has.
    #[wasm_bindgen]
    pub fn capabilities(&self) -> js_sys::Array {
        let arr = js_sys::Array::new();
        for cap in self.0.capabilities() {
            arr.push(&wasm_bindgen::JsValue::from_str(&cap.to_string()));
        }
        arr
    }
}
