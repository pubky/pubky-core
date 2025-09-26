// js/src/wrappers/session.rs
use wasm_bindgen::prelude::*;

use crate::js_error::PubkyJsError;
use crate::js_result::JsResult;
use crate::wrappers::session_info::SessionInfo;

#[wasm_bindgen]
pub struct Session(pub(crate) pubky::PubkySession);

#[wasm_bindgen]
impl Session {
    /// Return the session's user PublicKey.
    #[wasm_bindgen(js_name = "publicKey")]
    pub fn public_key(&self) -> crate::wrappers::keys::PublicKey {
        self.0.info().public_key().clone().into()
    }

    /// Return `SessionInfo``.
    #[wasm_bindgen]
    pub fn info(&self) -> SessionInfo {
        SessionInfo(self.0.info().clone())
    }

    /// Session-scoped storage (absolute paths).
    #[wasm_bindgen]
    pub fn storage(&self) -> crate::storage::SessionStorage {
        crate::storage::SessionStorage(pubky::SessionStorage::new(&self.0))
    }

    /// Invalidate this session on the server. Consumes the JS object.
    #[wasm_bindgen]
    pub async fn signout(self) -> JsResult<()> {
        match self.0.signout().await {
            Ok(()) => Ok(()),
            Err((e, _s)) => Err(PubkyJsError::from(e)),
        }
    }
}

impl From<pubky::PubkySession> for Session {
    fn from(s: pubky::PubkySession) -> Self {
        Session(s)
    }
}
