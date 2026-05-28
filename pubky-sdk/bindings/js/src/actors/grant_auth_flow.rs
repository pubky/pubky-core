use pubky::{GrantAuthFlowState, PubkyGrantAuthFlow};
use pubky_common::{auth::jws::ClientId, capabilities::Capabilities};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
use tsify::Tsify;
use url::Url;

use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::{auth_flow::AuthFlowKind, in_flight::InFlightGuard, session::Session};
use crate::{
    js_error::{JsResult, PubkyError, PubkyErrorName},
    wrappers::capabilities::validate_caps_for_start,
};

/// Options for starting a grant-backed pubkyauth flow.
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct GrantAuthFlowOptions {
    /// App identifier shown in the user's grant/session list, typically a domain.
    pub(crate) client_id: String,
    /// Optional HTTP relay base, e.g. `"https://demo.httprelay.io/inbox/"`.
    #[tsify(optional, type = "string | null")]
    pub(crate) relay: Option<String>,
}

/// Start and control a grant-backed pubkyauth authorization flow.
///
/// Typical flow:
/// 1) `GrantAuthFlow.start(...)` or `pubky.startGrantAuthFlow(...)`
/// 2) Show `authorizationUrl()` as QR/deeplink to the user's signing device
/// 3) `awaitApproval()` to receive a grant-backed, self-refreshing `Session`
#[wasm_bindgen]
pub struct GrantAuthFlow {
    inner: RefCell<Option<Rc<PubkyGrantAuthFlow>>>,
    in_flight: RefCell<bool>,
    authorization_url: String,
}

#[wasm_bindgen]
impl GrantAuthFlow {
    /// Start a grant-backed flow (standalone).
    /// Prefer `pubky.startGrantAuthFlow()` to reuse a facade client.
    ///
    /// @param {string} capabilities
    /// Comma-separated capabilities, e.g. `"/pub/app/:rw,/priv/foo.txt:r"`.
    /// Empty string is allowed (no scopes).
    ///
    /// @param {AuthFlowKind} kind
    /// The kind of authentication flow to perform.
    ///
    /// @param {GrantAuthFlowOptions} options
    /// Options for the grant flow: `{ clientId, relay? }`.
    ///
    /// @returns {GrantAuthFlow}
    /// A running grant auth flow. Call `authorizationUrl()` to show the deep link,
    /// then `awaitApproval()` to receive a grant-backed `Session`.
    #[wasm_bindgen(js_name = "start")]
    pub fn start(
        #[wasm_bindgen(unchecked_param_type = "Capabilities")] capabilities: String,
        kind: AuthFlowKind,
        options: GrantAuthFlowOptions,
    ) -> JsResult<GrantAuthFlow> {
        Self::start_with_client(capabilities, kind, options, None)
    }

    /// Internal helper that threads an explicit transport.
    pub(crate) fn start_with_client(
        capabilities: String,
        kind: AuthFlowKind,
        options: GrantAuthFlowOptions,
        client: Option<pubky::PubkyHttpClient>,
    ) -> JsResult<GrantAuthFlow> {
        let normalized = validate_caps_for_start(capabilities.as_str())?;
        let caps = Capabilities::try_from(normalized.as_str())?;
        let client_id = ClientId::new(&options.client_id).map_err(|e| {
            PubkyError::from(pubky::Error::Authentication(
                pubky::errors::AuthError::Validation(e.to_string()),
            ))
        })?;

        let mut builder = PubkyGrantAuthFlow::builder(&caps, kind.0, client_id);
        if let Some(c) = client {
            builder = builder.client(c);
        }

        if let Some(r) = options.relay {
            builder = builder.relay(Url::parse(&r)?);
        }

        let flow = builder.start()?;
        Ok(flow.into())
    }

    /// Resume a previously saved pending grant auth flow (standalone).
    /// Prefer `pubky.resumeGrantAuthFlow()` to reuse a facade client.
    ///
    /// **Security:** `savedState` contains the relay secret and PoP client private key.
    /// Store it only temporarily and delete it once the flow completes or is abandoned.
    ///
    /// @param {string} savedState A string produced by `grantFlow.save()`.
    /// @returns {GrantAuthFlow} A flow reconnected to the original relay channel.
    #[wasm_bindgen(js_name = "resume")]
    pub fn resume(saved_state: String) -> JsResult<GrantAuthFlow> {
        Self::resume_with_client(saved_state, None)
    }

    /// Internal helper that threads an explicit transport for resume.
    pub(crate) fn resume_with_client(
        saved_state: String,
        client: Option<pubky::PubkyHttpClient>,
    ) -> JsResult<GrantAuthFlow> {
        let state: GrantAuthFlowState = serde_json::from_str(&saved_state).map_err(|e| {
            PubkyError::new(
                PubkyErrorName::InvalidInput,
                format!("Invalid grant auth flow state: {e}"),
            )
        })?;
        let client = match client {
            Some(c) => c,
            None => pubky::PubkyHttpClient::new()?,
        };
        Ok(PubkyGrantAuthFlow::restore(state, client)?.into())
    }

    /// Return the authorization deep link (URL) to show as QR or open on the signer device.
    #[wasm_bindgen(js_name = "authorizationUrl", getter)]
    pub fn authorization_url(&self) -> String {
        self.authorization_url.clone()
    }

    /// Save sensitive state required to resume this pending grant flow.
    ///
    /// @returns {string} Opaque state for `GrantAuthFlow.resume()` or
    /// `pubky.resumeGrantAuthFlow()`.
    #[wasm_bindgen]
    pub fn save(&self) -> JsResult<String> {
        let flow = self.borrow_inner()?;
        serde_json::to_string(&flow.save()).map_err(|e| {
            PubkyError::new(
                PubkyErrorName::InternalError,
                format!("Failed to serialize grant auth flow state: {e}"),
            )
        })
    }

    /// Block until the user approves on their signer device; returns a grant-backed `Session`.
    #[wasm_bindgen(js_name = "awaitApproval")]
    pub async fn await_approval(&self) -> JsResult<Session> {
        let _guard = self.begin_call("awaitApproval")?;
        let flow = self.take_inner("awaitApproval")?;

        match Rc::try_unwrap(flow) {
            Ok(flow) => Ok(Session(flow.await_approval().await?)),
            Err(flow) => {
                self.restore_inner(flow);
                Err(self.in_use_error("awaitApproval"))
            }
        }
    }

    /// Non-blocking single poll step (advanced UIs).
    ///
    /// @returns {Promise<Session|undefined>} A session if the approval arrived, otherwise `undefined`.
    #[wasm_bindgen(js_name = "tryPollOnce")]
    pub async fn try_poll_once(&self) -> JsResult<Option<Session>> {
        let _guard = self.begin_call("tryPollOnce")?;
        let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
        let flow = self.borrow_inner()?;
        let result = flow.try_poll_once().await?.map(Session);
        let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
        Ok(result)
    }
}

impl From<PubkyGrantAuthFlow> for GrantAuthFlow {
    fn from(flow: PubkyGrantAuthFlow) -> Self {
        let auth_url = flow.authorization_url().as_str().to_string();
        GrantAuthFlow {
            authorization_url: auth_url,
            in_flight: RefCell::new(false),
            inner: RefCell::new(Some(Rc::new(flow))),
        }
    }
}

impl GrantAuthFlow {
    fn begin_call(&self, caller: &str) -> JsResult<InFlightGuard<'_>> {
        InFlightGuard::begin(&self.in_flight, || self.in_use_error(caller))
    }

    fn borrow_inner(&self) -> JsResult<Rc<PubkyGrantAuthFlow>> {
        self.inner
            .borrow()
            .as_ref()
            .cloned()
            .ok_or_else(|| self.consumed_error())
    }

    fn take_inner(&self, caller: &str) -> JsResult<Rc<PubkyGrantAuthFlow>> {
        self.inner
            .borrow_mut()
            .take()
            .ok_or_else(|| self.already_taken_error(caller))
    }

    fn restore_inner(&self, flow: Rc<PubkyGrantAuthFlow>) {
        let mut inner = self.inner.borrow_mut();
        *inner = Some(flow);
    }

    fn consumed_error(&self) -> PubkyError {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            "GrantAuthFlow has already completed; start a new flow for another login.",
        )
    }

    fn already_taken_error(&self, caller: &str) -> PubkyError {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            format!("GrantAuthFlow.{caller}() was already called; create a new GrantAuthFlow."),
        )
    }

    fn in_use_error(&self, caller: &str) -> PubkyError {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            format!("GrantAuthFlow.{caller}() cannot run while another call is in-flight."),
        )
    }
}
