use pubky_common::session;

use wasm_bindgen::prelude::*;

use super::keys::PublicKey;

/// Static snapshot of session metadata.
#[wasm_bindgen]
pub struct SessionInfo(pub(crate) session::SessionInfo);

#[wasm_bindgen]
impl SessionInfo {
    /// The user’s public key for this session.
    ///
    /// Use `.toString()` on the returned `PublicKey` to get the `pubky<z32>` identifier.
    /// Call `.z32()` when you specifically need the raw z-base32 value (e.g. hostnames).
    ///
    /// @returns {PublicKey}
    ///
    /// @example
    /// const who = sessionInfo.publicKey.toString();
    #[wasm_bindgen(js_name = "publicKey", getter)]
    pub fn public_key(&self) -> PublicKey {
        self.0.public_key().clone().into()
    }

    /// Effective capabilities granted to this session.
    ///
    /// @returns {string[]} Normalized capability entries (e.g. `"/pub/app/:rw"`).
    #[wasm_bindgen(js_name = "capabilities", getter)]
    pub fn capabilities(&self) -> Vec<String> {
        self.0
            .capabilities()
            .iter()
            .map(|c| c.to_string())
            .collect()
    }
}
