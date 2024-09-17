use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;

use url::Url;

use pubky_common::capabilities::Capabilities;

use crate::error::Error;
use crate::PubkyClient;

mod http;
mod keys;
mod pkarr;
mod recovery_file;
mod session;

use keys::{Keypair, PublicKey};
use session::Session;

impl Default for PubkyClient {
    fn default() -> Self {
        Self::new()
    }
}

static DEFAULT_RELAYS: [&str; 1] = ["https://relay.pkarr.org"];
static TESTNET_RELAYS: [&str; 1] = ["http://localhost:15411/pkarr"];

#[wasm_bindgen]
impl PubkyClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            session_cookies: Arc::new(RwLock::new(HashSet::new())),
            pkarr_relays: DEFAULT_RELAYS.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Create a client with with configurations appropriate for local testing:
    /// - set Pkarr relays to `["http://localhost:15411/pkarr"]` instead of default relay.
    #[wasm_bindgen]
    pub fn testnet() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            session_cookies: Arc::new(RwLock::new(HashSet::new())),
            pkarr_relays: TESTNET_RELAYS.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Set Pkarr relays used for publishing and resolving Pkarr packets.
    ///
    /// By default, [PubkyClient] will use `["https://relay.pkarr.org"]`
    #[wasm_bindgen(js_name = "setPkarrRelays")]
    pub fn set_pkarr_relays(mut self, relays: Vec<String>) -> Self {
        self.pkarr_relays = relays;
        self
    }

    // Read the set of pkarr relays used by this client.
    #[wasm_bindgen(js_name = "getPkarrRelays")]
    pub fn get_pkarr_relays(&self) -> Vec<String> {
        self.pkarr_relays.clone()
    }

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

    // === Public data ===

    #[wasm_bindgen]
    /// Upload a small payload to a given path.
    pub async fn put(&self, url: &str, content: &[u8]) -> Result<(), JsValue> {
        self.inner_put(url, content).await.map_err(|e| e.into())
    }

    /// Download a small payload from a given path relative to a pubky author.
    #[wasm_bindgen]
    pub async fn get(&self, url: &str) -> Result<Option<Uint8Array>, JsValue> {
        self.inner_get(url)
            .await
            .map(|b| b.map(|b| (&*b).into()))
            .map_err(|e| e.into())
    }

    /// Delete a file at a path relative to a pubky author.
    #[wasm_bindgen]
    pub async fn delete(&self, url: &str) -> Result<(), JsValue> {
        self.inner_delete(url).await.map_err(|e| e.into())
    }

    /// Returns a list of Pubky urls (as strings).
    ///
    /// - `url`:     The Pubky url (string) to the directory you want to list its content.
    /// - `cursor`:  Either a full `pubky://` Url (from previous list response),
    ///                 or a path (to a file or directory) relative to the `url`
    /// - `reverse`: List in reverse order
    /// - `limit`    Limit the number of urls in the response
    /// - `shallow`: List directories and files, instead of flat list of files.
    #[wasm_bindgen]
    pub async fn list(
        &self,
        url: &str,
        cursor: Option<String>,
        reverse: Option<bool>,
        limit: Option<u16>,
        shallow: Option<bool>,
    ) -> Result<Array, JsValue> {
        // TODO: try later to return Vec<String> from async function.

        if let Some(cursor) = cursor {
            return self
                .inner_list(url)?
                .reverse(reverse.unwrap_or(false))
                .limit(limit.unwrap_or(u16::MAX))
                .cursor(&cursor)
                .shallow(shallow.unwrap_or(false))
                .send()
                .await
                .map(|urls| {
                    let js_array = Array::new();

                    for url in urls {
                        js_array.push(&JsValue::from_str(&url));
                    }

                    js_array
                })
                .map_err(|e| e.into());
        }

        self.inner_list(url)?
            .reverse(reverse.unwrap_or(false))
            .limit(limit.unwrap_or(u16::MAX))
            .shallow(shallow.unwrap_or(false))
            .send()
            .await
            .map(|urls| {
                let js_array = Array::new();

                for url in urls {
                    js_array.push(&JsValue::from_str(&url));
                }

                js_array
            })
            .map_err(|e| e.into())
    }
}
