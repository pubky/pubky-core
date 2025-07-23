//! Wasm bindings for the Auth api

use url::Url;

use pubky_common::capabilities::Capabilities;

use crate::{
    js_result::JsResult,
    wrappers::{
        keys::{Keypair, PublicKey},
        session::Session,
    },
};

use super::super::constructor::Client;

use wasm_bindgen::prelude::*;

/// Optional parameters for the `signup` method.
#[wasm_bindgen]
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct SignupOptions {
    /// The signup token or invite code.
    #[wasm_bindgen(getter_with_clone, js_name = signupToken)]
    #[serde(rename = "signupToken")]
    pub signup_token: Option<String>,

    /// A boolean indicating acceptance of the Terms of Service.
    #[wasm_bindgen(js_name = acceptTos)]
    #[serde(rename = "acceptTos")]
    pub accept_tos: Option<bool>,
}

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
        options: JsValue,
    ) -> JsResult<Session> {
        let options: SignupOptions = if options.is_null() || options.is_undefined() {
            SignupOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options)?
        };

        // Start the native signup request builder.
        let mut signup_request = self.0.signup(keypair.as_inner(), homeserver.as_inner());

        // Conditionally add the signup token if it was provided.
        if let Some(token) = options.signup_token.as_deref() {
            signup_request = signup_request.with_signup_token(token);
        }

        // Conditionally accept ToS if the flag is true.
        if let Some(true) = options.accept_tos {
            signup_request = signup_request.accept_tos();
        }

        // Execute the configured request and map the result for the Wasm boundary.
        let session = signup_request
            .send()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(Session(session))
    }

    /// Check the current session for a given Pubky in its homeserver.
    ///
    /// Returns [Session] or `None` (if received `404 NOT_FOUND`),
    /// or throws the received error if the response has any other `>=400` status code.
    #[wasm_bindgen]
    pub async fn session(&self, pubky: &PublicKey) -> JsResult<Option<Session>> {
        self.0
            .session(pubky.as_inner())
            .await
            .map(|s| s.map(Session))
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Signout from a homeserver.
    #[wasm_bindgen]
    pub async fn signout(&self, pubky: &PublicKey) -> JsResult<()> {
        self.0
            .signout(pubky.as_inner())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Signin to a homeserver using the root Keypair.
    #[wasm_bindgen]
    pub async fn signin(&self, keypair: &Keypair) -> JsResult<()> {
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
    pub fn auth_request(&self, relay: &str, capabilities: &str) -> JsResult<AuthRequest> {
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
    pub async fn send_auth_token(&self, keypair: &Keypair, pubkyauth_url: &str) -> JsResult<()> {
        let pubkyauth_url: Url = pubkyauth_url.try_into().map_err(|_| "Invalid relay Url")?;

        self.0
            .send_auth_token(keypair.as_inner(), &pubkyauth_url)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }

    /// Get the homeserver id for a given Pubky public key.
    /// Looks up the pkarr packet for the given public key and returns the content of the first `_pubky` SVCB record.
    /// Throws an error if no homeserver is found.
    #[wasm_bindgen(js_name = "getHomeserver")]
    pub async fn get_homeserver(&self, public_key: &PublicKey) -> JsResult<PublicKey> {
        let hs_z32 = match self.0.get_homeserver(public_key.as_inner()).await {
            Some(hs_z32) => hs_z32,
            None => return Err(JsValue::from_str("No homeserver found")),
        };
        PublicKey::try_from(hs_z32)
    }

    /// Republish the user's PKarr record pointing to their homeserver.
    ///
    /// This method will republish the record if no record exists or if the existing record
    /// is older than 6 hours.
    ///
    /// The method is intended for clients and key managers (e.g., pubky-ring) to
    /// keep the records of active users fresh and available in the DHT and relays.
    /// It is intended to be used only after failed signin due to homeserver resolution
    /// failure. This method is lighter than performing a re-signup into the last known
    /// homeserver, but does not return a session token, so a signin must be done after
    /// republishing. On a failed signin due to homeserver resolution failure, a key
    /// manager should always attempt to republish the last known homeserver.
    #[wasm_bindgen(js_name = "republishHomeserver")]
    pub async fn republish_homeserver(&self, keypair: &Keypair, host: &PublicKey) -> JsResult<()> {
        self.0
            .republish_homeserver(keypair.as_inner(), host.as_inner())
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }
}

#[wasm_bindgen]
pub struct AuthRequest(pubky::AuthRequest);

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
    pub async fn response(&self) -> JsResult<PublicKey> {
        self.0
            .response()
            .await
            .map(PublicKey::from)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
