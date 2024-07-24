use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Keypair(pkarr::Keypair);

#[wasm_bindgen]
impl Keypair {
    #[wasm_bindgen]
    pub fn from_secret_key(secret_key: js_sys::Uint8Array) -> Self {
        let mut bytes = [0; 32];
        secret_key.copy_to(&mut bytes);

        Self(pkarr::Keypair::from_secret_key(&bytes))
    }
}
