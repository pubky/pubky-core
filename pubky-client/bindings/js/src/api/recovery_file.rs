use js_sys::Uint8Array;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{js_result::JsResult, wrappers::keys::Keypair};

/// Create a recovery file of the `keypair`, containing the secret key encrypted
/// using the `passphrase`.
#[wasm_bindgen(js_name = "createRecoveryFile")]
pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Uint8Array {
    pubky_common::recovery_file::create_recovery_file(keypair.as_inner(), passphrase)
        .as_slice()
        .into()
}

/// Create a recovery file of the `keypair`, containing the secret key encrypted
/// using the `passphrase`.
#[wasm_bindgen(js_name = "decryptRecoveryFile")]
pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> JsResult<Keypair> {
    let native_keypair =
        pubky_common::recovery_file::decrypt_recovery_file(recovery_file, passphrase)?;
    Ok(Keypair::from(native_keypair))
}
