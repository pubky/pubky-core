// js/src/wrappers/session.rs
use wasm_bindgen::prelude::*;

use super::storage::SessionStorage;
use crate::client::constructor::Client;
use crate::js_error::{JsResult, PubkyJsError};
use crate::wrappers::session_info::SessionInfo;

/// An authenticated context “as the user”.
/// - Use `storage()` for reads/writes (absolute paths like `/pub/app/file.txt`)
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

    /// Export a compact session secret that can be cached and restored later.
    ///
    /// The returned string is a bearer credential containing the public key and
    /// session cookie for this user. Store it securely (e.g. encrypted at rest)
    /// and avoid logging or copying it into shared channels.
    ///
    /// @returns {string}
    ///
    /// @example
    /// const secret = session.exportSecret();
    /// await localStorage.setItem("pubky-session", secret);
    // #[wasm_bindgen(js_name = "exportSecret")]
    // pub fn export_secret(&self) -> String {
    //     self.0.export_secret()
    // }

    // /// Restore a previously exported session secret.
    // ///
    // /// Performs a `/session` validation round-trip; if the secret is expired or
    // /// revoked the returned promise rejects with `{ name: "AuthenticationError" }`.
    // ///
    // /// @param {string} token Secret returned by {@link Session#exportSecret}.
    // /// @param {Client=} client Optional HTTP client to reuse cookies and relay
    // ///                         configuration. Defaults to a new client.
    // /// @returns {Promise<Session>}
    // ///
    // /// @example
    // /// const token = await localStorage.getItem("pubky-session");
    // /// const session = await Session.importSecret(token, pubky.client);
    // #[wasm_bindgen(js_name = "importSecret")]
    // pub async fn import_secret(token: String, client: Option<Client>) -> JsResult<Session> {
    //     let session = pubky::PubkySession::import_secret(&token, client.map(|c| c.0)).await?;
    //     Ok(Session(session))
    // }

    /// Invalidate the session on the server (clears server cookie).
    /// Further calls to storage API will fail.
    ///
    /// @returns {Promise<void>}
    #[wasm_bindgen]
    pub async fn signout(&self) -> JsResult<()> {
        match self.0.clone().signout().await {
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
