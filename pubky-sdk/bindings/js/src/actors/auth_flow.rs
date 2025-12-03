use pubky_common::capabilities::Capabilities;
use std::{cell::RefCell, rc::Rc};
use url::Url;

use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::session::Session;
use crate::{
    js_error::{JsResult, PubkyError, PubkyErrorName},
    wrappers::{auth_token::AuthToken, capabilities::validate_caps_for_start, keys::PublicKey},
};

/// Start and control a pubkyauth authorization flow.
///
/// Typical flow:
/// 1) `AuthFlow.start(...)` or `pubky.startAuthFlow(...)`
/// 2) Show `authorizationUrl()` as QR/deeplink to the user’s signing device
/// 3) `awaitApproval()` to receive a ready `Session`
#[wasm_bindgen]
pub struct AuthFlow {
    inner: RefCell<Option<Rc<pubky::PubkyAuthFlow>>>,
    in_flight: RefCell<bool>,
    authorization_url: String,
}

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
    /// @param {AuthFlowKind} kind
    /// The kind of authentication flow to perform.
    /// This can either be a sign in or a sign up flow.
    /// Examples:
    /// - `AuthFlowKind.signin()` - Sign in to an existing account.
    /// - `AuthFlowKind.signup(homeserverPublicKey, signupToken)` - Sign up for a new account.
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
        kind: AuthFlowKind,
        relay: Option<String>,
    ) -> JsResult<AuthFlow> {
        Self::start_with_client(capabilities, kind, relay, None)
    }

    /// Internal helper that threads an explicit transport.
    pub(crate) fn start_with_client(
        capabilities: String,
        kind: AuthFlowKind,
        relay: Option<String>,
        client: Option<pubky::PubkyHttpClient>,
    ) -> JsResult<AuthFlow> {
        // 1) Validate & normalize capability string
        let normalized = validate_caps_for_start(capabilities.as_str())?;
        // 2) Build native Capabilities
        let caps = Capabilities::try_from(normalized.as_str())?;

        // 3) Build the flow with optional relay and optional client
        let mut builder = pubky::PubkyAuthFlow::builder(&caps, kind.0);
        if let Some(c) = client {
            builder = builder.client(c);
        }

        if let Some(r) = relay {
            builder = builder.relay(Url::parse(&r)?);
        }

        let flow = builder.start()?;
        let auth_url = flow.authorization_url().as_str().to_string();

        Ok(AuthFlow {
            authorization_url: auth_url,
            in_flight: RefCell::new(false),
            inner: RefCell::new(Some(Rc::new(flow))),
        })
    }

    /// Return the authorization deep link (URL) to show as QR or open on the signer device.
    ///
    /// @returns {string} A `pubkyauth://…` or `https://…` URL with channel info.
    ///
    /// @example
    /// renderQr(flow.authorizationUrl());
    #[wasm_bindgen(js_name = "authorizationUrl", getter)]
    pub fn authorization_url(&self) -> String {
        self.authorization_url.clone()
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

    /// Block until the user approves on their signer device; returns an `AuthToken`.
    ///
    /// @returns {Promise<AuthToken>}
    /// Resolves when approved; rejects on timeout/cancel/network errors.
    ///
    /// @throws {PubkyError}
    /// - `RequestError` if relay/network fails
    #[wasm_bindgen(js_name = "awaitToken")]
    pub async fn await_token(&self) -> JsResult<AuthToken> {
        let _guard = self.begin_call("awaitToken")?;
        let flow = self.take_inner("awaitToken")?;

        match Rc::try_unwrap(flow) {
            Ok(flow) => Ok(AuthToken(flow.await_token().await?)),
            Err(flow) => {
                self.restore_inner(flow);
                Err(self.in_use_error("awaitToken"))
            }
        }
    }

    /// Non-blocking single poll step (advanced UIs).
    ///
    /// @returns {Promise<Session|undefined>} A session if the approval arrived, otherwise `undefined`.
    #[wasm_bindgen(js_name = "tryPollOnce")]
    pub async fn try_poll_once(&self) -> JsResult<Option<Session>> {
        let _guard = self.begin_call("tryPollOnce")?;
        // Ensure the in-flight guard spans at least one microtask so concurrent
        // calls (e.g., awaitApproval) observe the in-use state before this
        // probe settles.
        let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
        let flow = self.borrow_inner()?;
        let result = flow.try_poll_once().await?.map(Session);
        // Keep the guard alive for an extra turn so other calls racing this one
        // still see the in-flight flag even if the poll finishes immediately.
        let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
        Ok(result)
    }
}

impl AuthFlow {
    fn begin_call(&self, caller: &str) -> JsResult<InFlightGuard<'_>> {
        let mut flag = self.in_flight.borrow_mut();
        if *flag {
            Err(self.in_use_error(caller))
        } else {
            *flag = true;
            Ok(InFlightGuard {
                in_flight: &self.in_flight,
            })
        }
    }

    fn borrow_inner(&self) -> JsResult<Rc<pubky::PubkyAuthFlow>> {
        self.inner
            .borrow()
            .as_ref()
            .cloned()
            .ok_or_else(|| self.consumed_error())
    }

    fn take_inner(&self, caller: &str) -> JsResult<Rc<pubky::PubkyAuthFlow>> {
        self.inner
            .borrow_mut()
            .take()
            .ok_or_else(|| self.already_taken_error(caller))
    }

    fn restore_inner(&self, flow: Rc<pubky::PubkyAuthFlow>) {
        let mut inner = self.inner.borrow_mut();
        *inner = Some(flow);
    }

    fn consumed_error(&self) -> PubkyError {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            "AuthFlow has already completed; start a new flow for another login.",
        )
    }

    fn already_taken_error(&self, caller: &str) -> PubkyError {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            format!("AuthFlow.{caller}() was already called; create a new AuthFlow."),
        )
    }

    fn in_use_error(&self, caller: &str) -> PubkyError {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            format!("AuthFlow.{caller}() cannot run while another call is in-flight."),
        )
    }
}

struct InFlightGuard<'a> {
    in_flight: &'a RefCell<bool>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        let mut flag = self.in_flight.borrow_mut();
        *flag = false;
    }
}

/// The kind of authentication flow to perform.
/// This can either be a sign in or a sign up flow.
#[wasm_bindgen]
pub struct AuthFlowKind(pubky::AuthFlowKind);

#[wasm_bindgen]
impl AuthFlowKind {
    /// Create a sign in flow.
    #[wasm_bindgen(js_name = "signin")]
    pub fn signin() -> Self {
        Self(pubky::AuthFlowKind::SignIn)
    }

    /// Create a sign up flow.
    /// # Arguments
    /// * `homeserver_public_key` - The public key of the homeserver to sign up on.
    /// * `signup_token` - The signup token to use for the signup flow. This is optional.
    #[wasm_bindgen(js_name = "signup")]
    pub fn signup(homeserver_public_key: &PublicKey, signup_token: Option<String>) -> Self {
        Self(pubky::AuthFlowKind::SignUp {
            homeserver_public_key: Box::new(homeserver_public_key.0.to_owned()),
            signup_token,
        })
    }

    /// Get the intent of the authentication flow.
    /// # Returns
    /// * `"signin"` - If the authentication flow is a sign in flow.
    /// * `"signup"` - If the authentication flow is a sign up flow.
    #[wasm_bindgen(js_name = "intent", getter)]
    pub fn intent(&self) -> String {
        match &self.0 {
            pubky::AuthFlowKind::SignIn => "signin".to_string(),
            pubky::AuthFlowKind::SignUp { .. } => "signup".to_string(),
        }
    }
}
