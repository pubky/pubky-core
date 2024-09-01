use js_sys::Uint8Array;
use wasm_bindgen::prelude::wasm_bindgen;

/// Generate random bytes.
#[wasm_bindgen(js_name = "randomBytes")]
pub fn random_bytes(size: usize) -> Uint8Array {
    pubky_common::crypto::random_bytes(size).as_slice().into()
}

/// Pubky hash function (blake3)
#[wasm_bindgen]
pub fn hash(input: &[u8]) -> Uint8Array {
    pubky_common::crypto::hash(input)
        .as_bytes()
        .as_slice()
        .into()
}
