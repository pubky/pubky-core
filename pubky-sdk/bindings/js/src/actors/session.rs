// js/src/wrappers/session.rs
use wasm_bindgen::prelude::*;

use super::storage::SessionStorage;
use crate::client::constructor::Client;
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

    /// Export the session metadata so it can be restored after a tab refresh.
    ///
    /// The export string contains **no secrets**; it only serializes the public `SessionInfo`.
    /// Browsers remain responsible for persisting the HTTP-only session cookie.
    ///
    /// @returns {string}
    /// A base64 string to store (e.g. in `localStorage`).
    #[wasm_bindgen]
    pub fn export(&self) -> String {
        self.0.export()
    }

    /// Restore a session from an `export()` string.
    ///
    /// The HTTP-only cookie must still be present in the browser; this function does not
    /// read or write any secrets.
    ///
    /// @param {string} exported
    /// A string produced by `session.export()`.
    /// @param {Client=} client
    /// Optional client to reuse transport configuration.
    /// @returns {Promise<Session>}
    #[wasm_bindgen(js_name = "restore")]
    pub async fn restore(exported: String, client: Option<Client>) -> JsResult<Session> {
        let session = match client {
            Some(c) => pubky::PubkySession::import(&exported, Some(c.0)).await?,
            None => pubky::PubkySession::import(&exported, None).await?,
        };
        Ok(Session(session))
    }
}

impl From<pubky::PubkySession> for Session {
    fn from(s: pubky::PubkySession) -> Self {
        Session(s)
    }
}
