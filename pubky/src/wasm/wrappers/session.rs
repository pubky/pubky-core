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
    pub fn capabilities(&self) -> Vec<String> {
        self.0
            .capabilities()
            .iter()
            .map(|c| c.to_string())
            .collect()
    }
}
