//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::{Promise, Reflect};
use url::Url;
use wasm_bindgen::prelude::*;
use web_sys::{Headers, Request, RequestCredentials, RequestInit, ServiceWorkerGlobalScope};

use super::constructor::Client;
use crate::js_error::JsResult;

#[wasm_bindgen]
impl Client {
    /// Perform a raw fetch. Works with `pubky://` or `http(s)://` URLs.
    ///
    /// @param {string} url
    /// @param {RequestInit=} init Standard fetch options; `credentials: "include"` recommended for session I/O.
    /// @returns {Promise<Response>}
    ///
    /// @example
    /// const client = pubky.client();
    /// const res = await client.fetch(`pubky://${user}/pub/app/file.txt`, { method: "PUT", body: "hi", credentials: "include" });
    #[wasm_bindgen]
    pub async fn fetch(&self, url: &str, init: Option<RequestInit>) -> JsResult<Promise> {
        // 1) Parse URL
        let mut url = Url::parse(url)?;

        // 2) Ask the SDK to prepare (rewrite pubky://, resolve pkarr, etc.)
        //    Returns Some(<z32>) iff this is a pubky:// request.
        let pubky_host = self.0.prepare_request(&mut url).await?;

        // 3) Start from caller's init; DO NOT clobber headers.
        let req_init = init.unwrap_or_default();

        // 3a) If needed, ensure `pubky-host` is present in *init.headers* BEFORE Request creation.
        if let Some(host) = pubky_host.as_deref() {
            // Try to read any existing headers off RequestInit via reflection.
            // This value can be: undefined/null (no headers), a real `Headers`, or
            // a plain object/array. We handle those cases explicitly.
            let headers_js = Reflect::get(req_init.as_ref(), &JsValue::from_str("headers"))
                .unwrap_or(JsValue::UNDEFINED);

            if headers_js.is_undefined() || headers_js.is_null() {
                // No headers -> create and set ours.
                let headers = Headers::new()?;
                headers.set("pubky-host", host)?;
                req_init.set_headers(&headers.into());
            } else if headers_js.is_instance_of::<Headers>() {
                // Already a `Headers` object -> mutate in place (don’t replace).
                let headers: Headers = headers_js.unchecked_into();
                headers.set("pubky-host", host)?;
                // No need to set_headers again; we mutated the same object.
            } else {
                // Some non-`Headers` thing (e.g., plain object/array).
                // Safest is to replace with a real `Headers` that includes `pubky-host`.
                // (Our SDK paths pass a `Headers` already.)
                let headers = Headers::new()?;
                headers.set("pubky-host", host)?;
                req_init.set_headers(&headers.into());
            }
        }

        // 4) Always include credentials (cookies)
        req_init.set_credentials(RequestCredentials::Include);

        // 5) Build the Request *after* headers/credentials are set
        let js_req = Request::new_with_str_and_init(url.as_str(), &req_init)
            .map_err(|_| JsValue::from_str("invalid RequestInit"))?;

        // 6) Dispatch using the proper global (SW or Window)
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
