use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::RequestMode;

use pkarr::PkarrRelayClient;

use super::{keys::Keypair, PubkyClient};

#[wasm_bindgen]
impl PubkyClient {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    #[wasm_bindgen]
    pub async fn signup(&self, keypair: &Keypair, homeserver: &str) -> Result<String, JsValue> {
        let (audience, mut url) = self.resolve_endpoint(homeserver)?;

        url.set_path(&format!("/{}", keypair.public_key()));

        self.http
            .put(&url)
            .send_bytes(AuthnSignature::generate(keypair, &audience).as_bytes())?;

        self.publish_pubky_homeserver(keypair, homeserver).await;

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
