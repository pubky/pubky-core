//! Wasm bindings for the Auth api

use url::Url;

use pubky_common::capabilities::Capabilities;

use crate::wasm::wrappers::{
    keys::{Keypair, PublicKey},
    session::Session,
};

use super::super::Client;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl Client {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    #[wasm_bindgen]
    pub async fn signup(
        &self,
        keypair: &Keypair,
        homeserver: &PublicKey,
    ) -> Result<Session, JsValue> {
        Ok(Session(
            self.0
                .signup(keypair.as_inner(), homeserver.as_inner())
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?,
        ))
    }

    /// Check the current session for a given Pubky in its homeserver.
    ///
    /// Returns [Session] or `None` (if received `404 NOT_FOUND`),
    /// or throws the received error if the response has any other `>=400` status code.
    #[wasm_bindgen]
    pub async fn session(&self, pubky: &PublicKey) -> Result<Option<Session>, JsValue> {
        self.0
            .session(pubky.as_inner())
            .await
            .map(|s| s.map(Session))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Signout from a homeserver.
    #[wasm_bindgen]
    pub async fn signout(&self, pubky: &PublicKey) -> Result<(), JsValue> {
        self.0
            .signout(pubky.as_inner())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Signin to a homeserver using the root Keypair.
    #[wasm_bindgen]
    pub async fn signin(&self, keypair: &Keypair) -> Result<(), JsValue> {
        self.0
            .signin(keypair.as_inner())
            .await
            .map(|_| ())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Return `pubkyauth://` url and wait for the incoming [AuthToken]
    /// verifying that AuthToken, and if capabilities were requested, signing in to
    /// the Pubky's homeserver and returning the [Session] information.
    ///
    /// Returns a [AuthRequest]
    #[wasm_bindgen(js_name = "authRequest")]
    pub fn auth_request(&self, relay: &str, capabilities: &str) -> Result<AuthRequest, JsValue> {
        let auth_request = self
            .0
            .auth_request(
                relay,
                &Capabilities::try_from(capabilities).map_err(|_| "Invalid capaiblities")?,
            )
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(AuthRequest(auth_request))
    }

    /// Sign an [pubky_common::auth::AuthToken], encrypt it and send it to the
    /// source of the pubkyauth request url.
    #[wasm_bindgen(js_name = "sendAuthToken")]
    pub async fn send_auth_token(
        &self,
        keypair: &Keypair,
        pubkyauth_url: &str,
    ) -> Result<(), JsValue> {
        let pubkyauth_url: Url = pubkyauth_url.try_into().map_err(|_| "Invalid relay Url")?;

        self.0
            .send_auth_token(keypair.as_inner(), &pubkyauth_url)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }

    /// Sign an [pubky_common::auth::AuthToken], encrypt it and send it to the
    /// source of the pubkyauth request url.
    #[wasm_bindgen(js_name = "getHomeserver")]
    pub async fn get_homeserver(
        &self,
        public_key: &PublicKey,
    ) -> Result<(), JsValue> {
        let val = self.0.get_homeserver(public_key.as_inner()).await;
        let pubkyauth_url: Url = pubkyauth_url.try_into().map_err(|_| "Invalid relay Url")?;

        self.0
            .send_auth_token(keypair.as_inner(), &pubkyauth_url)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }
}

#[wasm_bindgen]
pub struct AuthRequest(crate::AuthRequest);

#[wasm_bindgen]
impl AuthRequest {
    /// Returns the Pubky Auth url, which you should show to the user
    /// to request an authentication or authorization token.
    ///
    /// Wait for this token using `this.response()`.
    #[wasm_bindgen]
    pub fn url(&self) -> String {
        self.0.url().as_str().to_string()
    }

    /// Wait for the user to send an authentication or authorization proof.
    ///
    /// If successful, you should expect an instance of [PublicKey]
    ///
    /// Otherwise it will throw an error.
    #[wasm_bindgen]
    pub async fn response(&self) -> Result<PublicKey, JsValue> {
        self.0
            .response()
            .await
            .map(PublicKey::from)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
