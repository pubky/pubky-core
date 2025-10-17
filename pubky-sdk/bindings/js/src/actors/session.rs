// js/src/wrappers/session.rs
use wasm_bindgen::prelude::*;

use super::storage::SessionStorage;
use crate::js_error::{JsResult, PubkyError};
use crate::wrappers::session_info::SessionInfo;

/// An authenticated context “as the user”.
/// - Use `storage` for reads/writes (absolute paths like `/pub/app/file.txt`)
/// - Cookie is managed automatically by the underlying fetch client
#[wasm_bindgen]
pub struct Session(pub(crate) pubky::PubkySession);

#[wasm_bindgen]
impl Session {
    /// Retrieve immutable info about this session (user & capabilities).
    ///
    /// @returns {SessionInfo}
    #[wasm_bindgen(getter)]
    pub fn info(&self) -> SessionInfo {
        SessionInfo(self.0.info().clone())
    }

    /// Access the session-scoped storage API (read/write).
    ///
    /// @returns {SessionStorage}
    #[wasm_bindgen(getter)]
    pub fn storage(&self) -> SessionStorage {
        SessionStorage(pubky::SessionStorage::new(&self.0))
    }

    /// Invalidate the session on the server (clears server cookie).
    /// Further calls to storage API will fail.
    ///
    /// @returns {Promise<void>}
    #[wasm_bindgen]
    pub async fn signout(&self) -> JsResult<()> {
        match self.0.clone().signout().await {
            Ok(()) => Ok(()),
            Err((e, _s)) => Err(PubkyError::from(e)),
        }
    }
}

impl From<pubky::PubkySession> for Session {
    fn from(s: pubky::PubkySession) -> Self {
        Session(s)
    }
}
