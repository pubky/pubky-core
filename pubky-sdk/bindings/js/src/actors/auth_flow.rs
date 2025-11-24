use pubky_common::capabilities::Capabilities;
use url::Url;
use wasm_bindgen::prelude::*;

use super::session::Session;
use crate::{
    js_error::JsResult,
    wrappers::{auth_token::AuthToken, capabilities::validate_caps_for_start},
};

/// Start and control a pubkyauth authorization flow.
///
/// Typical flow:
/// 1) `AuthFlow.start(...)` or `pubky.startAuthFlow(...)`
/// 2) Show `authorizationUrl()` as QR/deeplink to the user’s signing device
/// 3) `awaitApproval()` to receive a ready `Session`
#[wasm_bindgen]
pub struct AuthFlow(pub(crate) pubky::PubkyAuthFlow);

#[wasm_bindgen]
impl AuthFlow {
    /// Start a flow (standalone).
    /// Prefer `pubky.startAuthFlow()` to reuse a facade client.
    ///
    /// @param {string} capabilities
    /// Comma-separated capabilities, e.g. `"/pub/app/:rw,/priv/foo.txt:r"`.
    /// Each entry must be `"<scope>:<actions>"`, where:
    /// - `scope` starts with `/` (e.g. `/pub/example.com/`)
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
    /// @throws {PubkyError}
    /// - `{ name: "InvalidInput", message: string }` if any capability entry is invalid
    ///     or for an invalid relay URL.
    /// @example
    /// const flow = AuthFlow.start("/pub/my-cool-app/:rw,/pub/pubky.app/:w");
    /// renderQRCode(flow.authorizationUrl());
    /// const session = await flow.awaitApproval();
    #[wasm_bindgen(js_name = "start")]
    pub fn start(
        #[wasm_bindgen(unchecked_param_type = "Capabilities")] capabilities: String,
        relay: Option<String>,
    ) -> JsResult<AuthFlow> {
        Self::start_with_client(capabilities, relay, None)
    }

    /// Internal helper that threads an explicit transport.
    pub(crate) fn start_with_client(
        capabilities: String,
        relay: Option<String>,
        client: Option<pubky::PubkyHttpClient>,
    ) -> JsResult<AuthFlow> {
        // 1) Validate & normalize capability string
        let normalized = validate_caps_for_start(capabilities.as_str())?;
        // 2) Build native Capabilities
        let caps = Capabilities::try_from(normalized.as_str())?;

        // 3) Build the flow with optional relay and optional client
        let mut builder = pubky::PubkyAuthFlow::builder(&caps);
        if let Some(c) = client {
            builder = builder.client(c);
        }
        if let Some(r) = relay {
            builder = builder.base_relay(Url::parse(&r)?);
        }

        Ok(AuthFlow(builder.start()?))
    }

    /// Return the authorization deep link (URL) to show as QR or open on the signer device.
    ///
    /// @returns {string} A `pubkyauth://…` or `https://…` URL with channel info.
    ///
    /// @example
    /// renderQr(flow.authorizationUrl());
    #[wasm_bindgen(js_name = "authorizationUrl", getter)]
    pub fn authorization_url(&self) -> String {
        self.0.authorization_url().as_str().to_string()
    }

    /// Block until the user approves on their signer device; returns a `Session`.
    ///
    /// @returns {Promise<Session>}
    /// Resolves when approved; rejects on timeout/cancel/network errors.
    ///
    /// @throws {PubkyError}
    /// - `RequestError` if relay/network fails
    /// - `AuthenticationError` if approval is denied/invalid
    #[wasm_bindgen(js_name = "awaitApproval")]
    pub async fn await_approval(self) -> JsResult<Session> {
        Ok(Session(self.0.await_approval().await?))
    }

    /// Block until the user approves on their signer device; returns an `AuthToken`.
    ///
    /// @returns {Promise<AuthToken>}
    /// Resolves when approved; rejects on timeout/cancel/network errors.
    ///
    /// @throws {PubkyError}
    /// - `RequestError` if relay/network fails
    #[wasm_bindgen(js_name = "awaitToken")]
    pub async fn await_token(self) -> JsResult<AuthToken> {
        Ok(AuthToken(self.0.await_token().await?))
    }

    /// Non-blocking single poll step (advanced UIs).
    ///
    /// @returns {Promise<Session|undefined>} A session if the approval arrived, otherwise `undefined`.
    #[wasm_bindgen(js_name = "tryPollOnce")]
    pub async fn try_poll_once(&self) -> JsResult<Option<Session>> {
        Ok(self.0.try_poll_once().await?.map(Session))
    }
}
