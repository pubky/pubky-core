use js_sys::Uint8Array;
use wasm_bindgen::prelude::{wasm_bindgen, JsValue};

use crate::error::Error;

use super::keys::Keypair;

/// Create a recovery file of the `keypair`, containing the secret key encrypted
/// using the `passphrase`.
#[wasm_bindgen(js_name = "createRecoveryFile")]
pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Result<Uint8Array, JsValue> {
    pubky_common::recovery_file::create_recovery_file(keypair.as_inner(), passphrase)
        .map(|b| b.as_slice().into())
        .map_err(|e| Error::from(e).into())
}

/// Create a recovery file of the `keypair`, containing the secret key encrypted
/// using the `passphrase`.
#[wasm_bindgen(js_name = "decryptRecoveryFile")]
pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair, JsValue> {
    pubky_common::recovery_file::decrypt_recovery_file(recovery_file, passphrase)
        .map(Keypair::from)
        .map_err(|e| Error::from(e).into())
}
