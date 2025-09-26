use pubky_common::session;

use wasm_bindgen::prelude::*;

use super::keys::PublicKey;

#[wasm_bindgen]
pub struct SessionInfo(pub(crate) session::SessionInfo);

#[wasm_bindgen]
impl SessionInfo {
    /// Return the [PublicKey] of this session
    #[wasm_bindgen]
    pub fn public_key(&self) -> PublicKey {
        self.0.public_key().clone().into()
    }

    /// Return the capabilities that this session has.
    pub fn capabilities(&self) -> Vec<String> {
        self.0
            .capabilities()
            .iter()
            .map(|c| c.to_string())
            .collect()
    }
}
