use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::RequestMode;

use pkarr::PkarrRelayClient;

use super::Keypair;

#[wasm_bindgen]
pub struct Error {}

#[wasm_bindgen]
pub struct PubkyClient {
    pkarr: PkarrRelayClient,
}

#[wasm_bindgen]
impl PubkyClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            pkarr: PkarrRelayClient::default(),
        }
    }

    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    #[wasm_bindgen]
    pub fn signup(&self, secret_key: Keypair, homeserver: &str) -> Result<(), JsValue> {
        // let (audience, mut url) = self.resolve_endpoint(homeserver)?;

        // url.set_path(&format!("/{}", keypair.public_key()));

        // let body = AuthnSignature::generate(keypair, &audience).as_bytes();

        // fetch_base(url.to_string(), "PUT", body).await?;

        // self.publish_pubky_homeserver(keypair, homeserver);

        Ok(())
    }
}

async fn fetch_base(
    url: &String,
    method: &str,
    body: Option<Vec<u8>>,
) -> Result<web_sys::Response, JsValue> {
    let mut opts = web_sys::RequestInit::new();
    opts.method(method);
    opts.mode(RequestMode::Cors);

    if let Some(body) = body {
        let body_bytes: &[u8] = &body;
        let body_array: js_sys::Uint8Array = body_bytes.into();
        let js_value: &JsValue = body_array.as_ref();
        opts.body(Some(js_value));
    }

    let js_request = web_sys::Request::new_with_str_and_init(url, &opts)?;

    let window = web_sys::window().unwrap();
    let response = JsFuture::from(window.fetch_with_request(&js_request)).await?;

    let response: web_sys::Response = response.dyn_into()?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::*;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    use super::*;

    #[wasm_bindgen_test]
    async fn basic() {
        // let client = PubkyClient::new();
    }
}
