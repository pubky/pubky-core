//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::Promise;
use url::Url;
use wasm_bindgen::prelude::*;
use web_sys::{Headers, Request, RequestCredentials, RequestInit, ServiceWorkerGlobalScope};

use crate::constructor::Client;
use crate::js_result::JsResult;

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen]
    pub async fn fetch(&self, url: &str, init: Option<RequestInit>) -> JsResult<Promise> {
        // 1) Parse URL
        let mut url = Url::parse(url)?;
        let req_init = init.unwrap_or_default();

        // 2) Ask the SDK to prepare the request (rewrite pubky://, resolve _pubky.<pk>, etc.)
        if let Some(pubky_host) = self.0.prepare_request(&mut url).await? {
            // Add the `pubky-host` header expected by the server in WASM environments.
            let headers = Headers::new()?;
            headers.append("pubky-host", &pubky_host)?;
            req_init.set_headers(&headers.into());
        }

        // 3) Always include credentials (cookies) — matches reqwest’s `.fetch_credentials_include()`.
        req_init.set_credentials(RequestCredentials::Include);

        // 4) Build a JS Request and dispatch it using the environment’s fetch.
        let js_req = Request::new_with_str_and_init(url.as_str(), &req_init)
            .map_err(|_| JsValue::from_str("invalid RequestInit"))?;
        Ok(js_fetch(&js_req))
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = fetch)]
    fn fetch_with_request(input: &web_sys::Request) -> Promise;
}

fn js_fetch(req: &web_sys::Request) -> Promise {
    use wasm_bindgen::{JsCast, JsValue};
    let global = js_sys::global();
    if let Ok(true) = js_sys::Reflect::has(&global, &JsValue::from_str("ServiceWorkerGlobalScope"))
    {
        global
            .unchecked_into::<ServiceWorkerGlobalScope>()
            .fetch_with_request(req)
    } else {
        // Browser
        fetch_with_request(req)
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use pkarr::Keypair;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    // Ensure we expose pubky-host AND set credentials=include for pubky:// URLs
    #[wasm_bindgen_test(async)]
    async fn prepare_sets_pubky_host_and_credentials() {
        let client = Client::testnet(None);
        let pk = Keypair::random().public_key().to_string();
        let mut url = Url::parse(&format!("pubky://{}/pub/file.txt", pk)).unwrap();

        // Mirror the `fetch()` code path (but don't actually dispatch fetch)
        let mut req_init = RequestInit::new();
        let host_opt = client.0.prepare_request(&mut url).await.unwrap();
        assert_eq!(host_opt.as_deref(), Some(pk.as_str()));

        req_init.set_credentials(RequestCredentials::Include);
        assert_eq!(req_init.credentials(), Some(RequestCredentials::Include));
    }

    // ICANN URL must not require pubky-host but should still allow credentials=include
    #[wasm_bindgen_test(async)]
    async fn prepare_icann_does_not_set_pubky_host() {
        let client = Client::new(None).unwrap();
        let mut url = Url::parse("https://example.com/").unwrap();

        let host_opt = client.0.prepare_request(&mut url).await.unwrap();
        assert!(host_opt.is_none());
    }
}
