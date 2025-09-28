use pubky_common::capabilities::Capabilities;
use url::Url;
use wasm_bindgen::prelude::*;

use crate::{
    js_result::JsResult, session::Session, wrappers::capabilities::validate_caps_for_start,
};

/// JS-facing auth flow handle that polls a relay until a signer approves.
#[wasm_bindgen]
pub struct AuthFlow(pub(crate) pubky::PubkyAuthFlow);

#[wasm_bindgen]
impl AuthFlow {
    /// Start an auth flow.
    ///
    /// - `capabilities` - a string of comma-separated entries like:
    ///   `"/pub/app/:rw,/priv/foo.txt:r"`.
    ///   Each entry must be `<scope>:<actions>` where `actions` are any combo of `r` and/or `w`.
    ///   Examples:
    ///     - `"/":rw"` (root read/write)
    ///     - `"/pub/example.com/:r"`
    ///     - `"/pub/app/:wr"` -> normalized to `"/pub/app/:rw"`
    ///   If **any** entry is invalid, this throws an `InvalidInput` error with
    ///   `error.data = string[]` listing the invalid tokens.
    ///   An empty string is allowed (means “no requested scopes”).
    ///
    /// - `relay` - optional HTTP relay base, e.g. `"http://localhost:15412/link/"`.
    ///   If omitted, the default relay is used.
    ///
    /// Background polling starts immediately. Call `authorizationUrl()` to show the QR/deeplink,
    /// then `awaitApproval()` to get a `Session`, or poll via `tryPollOnce()`.
    #[wasm_bindgen(js_name = "start")]
    pub fn start(capabilities: &str, relay: Option<String>) -> JsResult<AuthFlow> {
        // 1) Validate & normalize the capabilities string (fail fast with details).
        let normalized = validate_caps_for_start(capabilities)?;

        // 2) Build native Capabilities (will be valid now).
        let caps = Capabilities::try_from(normalized.as_str())?;

        // 3) Build the flow (optionally overriding the relay).
        let flow = if let Some(r) = relay {
            let url = Url::parse(&r)?;
            pubky::PubkyAuthFlow::builder(caps).relay(url).start()?
        } else {
            pubky::PubkyAuthFlow::start(&caps)?
        };
        Ok(AuthFlow(flow))
    }

    /// The `pubkyauth://` deep link to show (QR/deeplink) to the signer.
    #[wasm_bindgen(js_name = "authorizationUrl")]
    pub fn authorization_url(&self) -> String {
        self.0.authorization_url().as_str().to_string()
    }

    /// Block until the signer approves; returns a ready `Session`.
    #[wasm_bindgen(js_name = "awaitApproval")]
    pub async fn await_approval(self) -> JsResult<Session> {
        Ok(Session(self.0.await_approval().await?))
    }

    /// Non-blocking probe; returns `Some(Session)` when ready, otherwise `undefined`.
    #[wasm_bindgen(js_name = "tryPollOnce")]
    pub async fn try_poll_once(&self) -> JsResult<Option<Session>> {
        Ok(self.0.try_poll_once().await?.map(Session))
    }
}
