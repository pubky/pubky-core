use pubky_common::auth::AuthnSignature;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::RequestMode;

use pkarr::PkarrRelayClient;

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
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use super::*;

    #[wasm_bindgen_test]
    async fn basic() {
        // let client = PubkyClient::new();
    }
}
