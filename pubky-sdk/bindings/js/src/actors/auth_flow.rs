use pubky_common::capabilities::Capabilities;
use url::Url;
use wasm_bindgen::prelude::*;

use super::session::Session;
use crate::{js_error::JsResult, wrappers::capabilities::validate_caps_for_start};

/// JS-facing auth flow handle that polls a relay until a signer approves.
#[wasm_bindgen]
pub struct AuthFlow(pub(crate) pubky::PubkyAuthFlow);

#[wasm_bindgen]
impl AuthFlow {
    /// Start an auth flow.
    ///
    /// @param {string} capabilities
    /// Comma-separated capabilities, e.g. `"/pub/app/:rw,/priv/foo.txt:r"`.
    /// Each entry must be `"<scope>:<actions>"`, where:
    /// - `scope` starts with `/` (e.g. `/pub/example.app/`)
    /// - `actions` is any combo of `r` and/or `w` (order is normalized; `wr` -> `rw`)
    /// Empty string is allowed (no scopes).
    ///
    /// @param {string} [relay]
    /// Optional HTTP relay base, e.g. `"https://demo.httprelay.io/link/"`.
    /// Defaults to the default Synonym-hosted relay when omitted.
    ///
    /// @returns {AuthFlow}
    /// A running auth flow. Call `authorizationUrl()` to show the deep link,
    /// then `awaitApproval()` to receive a `Session`.
    /// @throws {PubkyJsError}
    /// - `{ name: "InvalidInput", mesage: string }` if any capability entry is invalid
    ///     or for an invalid relay URL.
    /// @example
    /// const flow = AuthFlow.start("/pub/my.app/:rw,/pub/pubky.app/:w");
    /// renderQRCode(flow.authorizationUrl());
    /// const session = await flow.awaitApproval();
    #[wasm_bindgen(js_name = "start")]
    pub fn start(capabilities: &str, relay: Option<String>) -> JsResult<AuthFlow> {
        Self::start_with_client(capabilities, relay, None)
    }

    /// Rust-only helper that threads an explicit transport.
    pub(crate) fn start_with_client(
        capabilities: &str,
        relay: Option<String>,
        client: Option<pubky::PubkyHttpClient>,
    ) -> JsResult<AuthFlow> {
        // 1) Validate & normalize capability string
        let normalized = validate_caps_for_start(capabilities)?;
        // 2) Build native Capabilities
        let caps = Capabilities::try_from(normalized.as_str())?;

        // 3) Build the flow with optional relay and optional client
        let mut builder = pubky::PubkyAuthFlow::builder(&caps);
        if let Some(c) = client {
            builder = builder.client(c);
        }
        if let Some(r) = relay {
            builder = builder.relay(Url::parse(&r)?);
        }

        Ok(AuthFlow(builder.start()?))
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
