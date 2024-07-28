use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::RequestMode;

use reqwest::StatusCode;

use pkarr::PkarrRelayClient;

use pubky_common::{auth::AuthnSignature, session::Session};

use crate::Error;

use super::{
    keys::{Keypair, PublicKey},
    PubkyClient,
};

#[wasm_bindgen]
impl PubkyClient {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    #[wasm_bindgen]
    pub async fn signup(&self, keypair: &Keypair, homeserver: &PublicKey) -> Result<(), JsValue> {
        let keypair = keypair.as_inner();
        let homeserver = homeserver.as_inner().to_string();

        let (audience, mut url) = self.resolve_endpoint(&homeserver).await?;

        url.set_path(&format!("/{}", keypair.public_key()));

        let body = AuthnSignature::generate(keypair, &audience)
            .as_bytes()
            .to_owned();

        self.http.put(url).body(body).send().await?;

        self.publish_pubky_homeserver(keypair, &homeserver).await?;

        Ok(())
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns an [Error::NotSignedIn] if so, or [reqwest::Error] if
    /// the response has any other `>=400` status code.
    #[wasm_bindgen]
    pub async fn session(&self, pubky: &PublicKey) -> Result<Session, JsValue> {
        let (homeserver, mut url) = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{}/session", pubky));

        let res = self.http.get(url).send().await?;

        if res.status() == StatusCode::NOT_FOUND {
            return Err(Error::NotSignedIn);
        }

        if !res.status().is_success() {
            res.error_for_status_ref()?;
        };

        let bytes = res.bytes().await?;

        Ok(Session::deserialize(&bytes)?)
    }
}
