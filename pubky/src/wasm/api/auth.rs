//! Wasm bindings for the Auth api

use url::Url;

use pubky_common::capabilities::Capabilities;

use crate::Client;

use crate::Error;

use crate::wasm::wrappers::keys::{Keypair, PublicKey};
use crate::wasm::wrappers::session::Session;

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
            self.inner_signup(keypair.as_inner(), homeserver.as_inner())
                .await
                .map_err(JsValue::from)?,
        ))
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns [Session] or `None` (if recieved `404 NOT_FOUND`),
    /// or throws the recieved error if the response has any other `>=400` status code.
    #[wasm_bindgen]
    pub async fn session(&self, pubky: &PublicKey) -> Result<Option<Session>, JsValue> {
        self.inner_session(pubky.as_inner())
            .await
            .map(|s| s.map(Session))
            .map_err(|e| e.into())
    }

    /// Signout from a homeserver.
    #[wasm_bindgen]
    pub async fn signout(&self, pubky: &PublicKey) -> Result<(), JsValue> {
        self.inner_signout(pubky.as_inner())
            .await
            .map_err(|e| e.into())
    }

    /// Signin to a homeserver using the root Keypair.
    #[wasm_bindgen]
    pub async fn signin(&self, keypair: &Keypair) -> Result<(), JsValue> {
        self.inner_signin(keypair.as_inner())
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    }

    /// Return `pubkyauth://` url and wait for the incoming [AuthToken]
    /// verifying that AuthToken, and if capabilities were requested, signing in to
    /// the Pubky's homeserver and returning the [Session] information.
    ///
    /// Returns a tuple of [pubkyAuthUrl, Promise<Session>]
    #[wasm_bindgen(js_name = "authRequest")]
    pub fn auth_request(&self, relay: &str, capabilities: &str) -> Result<js_sys::Array, JsValue> {
        let mut relay: Url = relay
            .try_into()
            .map_err(|_| Error::Generic("Invalid relay Url".into()))?;

        let (pubkyauth_url, client_secret) = self.create_auth_request(
            &mut relay,
            &Capabilities::try_from(capabilities).map_err(|_| "Invalid capaiblities")?,
        )?;

        let this = self.clone();

        let future = async move {
            this.subscribe_to_auth_response(relay, &client_secret)
                .await
                .map(|pubky| JsValue::from(PublicKey(pubky)))
                .map_err(|err| JsValue::from_str(&format!("{:?}", err)))
        };

        let promise = wasm_bindgen_futures::future_to_promise(future);

        // Return the URL and the promise
        let js_tuple = js_sys::Array::new();
        js_tuple.push(&JsValue::from_str(pubkyauth_url.as_ref()));
        js_tuple.push(&promise);

        Ok(js_tuple)
    }

    /// Sign an [pubky_common::auth::AuthToken], encrypt it and send it to the
    /// source of the pubkyauth request url.
    #[wasm_bindgen(js_name = "sendAuthToken")]
    pub async fn send_auth_token(
        &self,
        keypair: &Keypair,
        pubkyauth_url: &str,
    ) -> Result<(), JsValue> {
        let pubkyauth_url: Url = pubkyauth_url
            .try_into()
            .map_err(|_| Error::Generic("Invalid relay Url".into()))?;

        self.inner_send_auth_token(keypair.as_inner(), pubkyauth_url)
            .await?;

        Ok(())
    }
}
