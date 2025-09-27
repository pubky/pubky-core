use pubky_common::capabilities::Capabilities;
use url::Url;
use wasm_bindgen::prelude::*;

use crate::{js_result::JsResult, session::Session};

/// JS-facing auth flow handle that polls a relay until a signer approves.
#[wasm_bindgen]
pub struct AuthFlow(pub(crate) pubky::PubkyAuthFlow);

#[wasm_bindgen]
impl AuthFlow {
    /// Start an auth flow.
    ///
    /// - `capabilities` — string accepted by `pubky_common::Capabilities::try_from`
    ///   (e.g. `"{}"` or `"r:w"`).
    /// - `relay` — optional custom HTTP relay base (e.g. `"http://localhost:8080/link/"`).
    ///
    /// Background polling starts immediately.
    #[wasm_bindgen(js_name = "start")]
    pub fn start(capabilities: &str, relay: Option<String>) -> JsResult<AuthFlow> {
        let caps = Capabilities::try_from(capabilities)?;
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
