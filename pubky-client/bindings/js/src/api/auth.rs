//! Wasm bindings for the Auth API.

use pubky_common::capabilities::Capabilities;
use wasm_bindgen::prelude::*;

use crate::{
    constructor::Client,
    http_client::WasmHttpClient,
    js_result::JsResult,
    wrappers::{
        keys::{Keypair, PublicKey},
        session::Session,
    },
};

#[wasm_bindgen]
impl Client {
    /// Signs up to a homeserver and updates the Pkarr record accordingly.
    ///
    /// The homeserver is identified by its Pkarr public key.
    /// @param {Keypair} keypair - The user's root keypair.
    /// @param {PublicKey} homeserver - The public key of the homeserver to sign up to.
    /// @param {string | undefined} signup_token - An optional invite token required by the server.
    /// @returns {Promise<Session>} A session object upon successful signup.
    /// @throws Will throw an error if the signup fails.
    #[wasm_bindgen]
    pub async fn signup(
        &self,
        keypair: &Keypair,
        homeserver: &PublicKey,
        signup_token: Option<String>,
    ) -> JsResult<Session> {
        self.inner
            .signup(
                keypair.as_inner(),
                homeserver.as_inner(),
                signup_token.as_deref(),
            )
            .await
            .map(Session)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Checks the current session for a given Pubky on its homeserver.
    ///
    /// @param {PublicKey} pubky - The public key to check the session for.
    /// @returns {Promise<Session | null>} The current session object, or `null` if not signed in.
    /// @throws Will throw an error for network issues or non-404 server errors.
    #[wasm_bindgen]
    pub async fn session(&self, pubky: &PublicKey) -> JsResult<Option<Session>> {
        self.inner
            .session(pubky.as_inner())
            .await
            .map(|opt_s| opt_s.map(Session))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Signs out from a homeserver.
    ///
    /// This invalidates the current session cookie for the given public key.
    /// @param {PublicKey} pubky - The public key to sign out.
    /// @returns {Promise<void>}
    /// @throws Will throw an error if the signout request fails.
    #[wasm_bindgen]
    pub async fn signout(&self, pubky: &PublicKey) -> JsResult<()> {
        self.inner
            .signout(pubky.as_inner())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Signs in to a homeserver using the root Keypair.
    ///
    /// This also ensures the user's Pkarr record is published or refreshed.
    /// @param {Keypair} keypair - The user's root keypair to sign in with.
    /// @returns {Promise<Session>} The new session object.
    /// @throws Will throw an error if the signin fails.
    #[wasm_bindgen]
    pub async fn signin(&self, keypair: &Keypair) -> JsResult<Session> {
        self.inner
            .signin(keypair.as_inner())
            .await
            .map(Session)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Creates an authentication request.
    ///
    /// Returns an `AuthRequest` object containing the `pubkyauth://` URL to show
    /// the user and a `response()` method to await their action.
    /// @param {string} relay - The URL of the relay server for the auth handshake.
    /// @param {string} capabilities - A comma-separated string of requested capabilities.
    /// @returns {AuthRequest} An object to manage the authentication flow.
    /// @throws Will throw if the capabilities string is invalid.
    #[wasm_bindgen(js_name = "authRequest")]
    pub fn auth_request(&self, relay: &str, capabilities: &str) -> JsResult<AuthRequest> {
        let caps =
            Capabilities::try_from(capabilities).map_err(|_| "Invalid capabilities string")?;

        let auth_request = self
            .inner
            .auth_request(relay, &caps)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(AuthRequest(auth_request))
    }

    /// Signs and sends an `AuthToken` in response to a `pubkyauth://` URL.
    ///
    /// @param {Keypair} keypair - The keypair to use for signing the auth token.
    /// @param {string} pubkyauth_url - The full `pubkyauth://` URL from the requesting application.
    /// @returns {Promise<void>}
    /// @throws Will throw if the URL is invalid or the request fails.
    #[wasm_bindgen(js_name = "sendAuthToken")]
    pub async fn send_auth_token(&self, keypair: &Keypair, pubkyauth_url: &str) -> JsResult<()> {
        self.inner
            .send_auth_token(keypair.as_inner(), pubkyauth_url)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Gets the homeserver ID for a given Pubky public key by resolving its Pkarr record.
    ///
    /// @param {PublicKey} public_key - The public key to resolve.
    /// @returns {Promise<PublicKey>} The public key of the homeserver.
    /// @throws Will throw an error if no homeserver is found.
    #[wasm_bindgen(js_name = "getHomeserver")]
    pub async fn get_homeserver(&self, public_key: &PublicKey) -> JsResult<PublicKey> {
        match self.inner.get_homeserver(public_key.as_inner()).await {
            Some(hs_z32) => PublicKey::try_from(hs_z32),
            None => Err(JsValue::from_str(
                "No homeserver found for the given public key",
            )),
        }
    }

    /// Republishes the user's Pkarr record pointing to their homeserver.
    ///
    /// This method is intended for clients and key managers to keep the records of
    /// active users fresh in the DHT, especially after a failed sign-in due to
    /// homeserver resolution failure. It is lighter than a full re-signup but does
    /// not return a session; a sign-in must be performed afterward.
    /// @param {Keypair} keypair - The user's keypair to sign the Pkarr record.
    /// @param {PublicKey} host - The public key of the homeserver to point the record to.
    /// @returns {Promise<void>}
    /// @throws Will throw if the publication to the DHT fails.
    #[wasm_bindgen(js_name = "republishHomeserver")]
    pub async fn republish_homeserver(&self, keypair: &Keypair, host: &PublicKey) -> JsResult<()> {
        self.inner
            .republish_homeserver(keypair.as_inner(), host.as_inner())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

/// A wrapper for a pending authentication request.
#[wasm_bindgen]
pub struct AuthRequest(pubky::AuthRequest<WasmHttpClient>);

#[wasm_bindgen]
impl AuthRequest {
    /// Returns the `pubkyauth://` URL that should be presented to the user.
    #[wasm_bindgen]
    pub fn url(&self) -> String {
        self.0.url().to_string()
    }

    /// Asynchronously waits for the user to respond to the authentication request.
    ///
    /// If successful, it resolves with the `PublicKey` of the authenticating user.
    /// @returns {Promise<PublicKey>}
    /// @throws Will throw an error if the authentication fails or times out.
    #[wasm_bindgen]
    pub async fn response(&self) -> JsResult<PublicKey> {
        self.0
            .response()
            .await
            .map(PublicKey::from)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
