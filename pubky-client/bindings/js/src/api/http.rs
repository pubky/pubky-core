//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::Promise;
use url::Url;
use wasm_bindgen::prelude::*;
use web_sys::{Request, RequestInit, ServiceWorkerGlobalScope};

use crate::constructor::Client;
use crate::js_result::JsResult;

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen]
    pub async fn fetch(&self, url: &str, init: Option<RequestInit>) -> JsResult<Promise> {
        // 1. parse
        let mut url = Url::parse(url).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let req_init = init.unwrap_or_default();

        // 2. add pubky-host query string if needed
        if let Some(host) = self.0.prepare_request(&mut url).await {
            if url.query_pairs().any(|(k, _)| k != "pubky-host") {
                url.query_pairs_mut().append_pair("pubky-host", &host);
            };
        }
        // 3. build JS Request
        let js_req = Request::new_with_str_and_init(url.as_str(), &req_init)
            .map_err(|_| JsValue::from_str("invalid RequestInit"))?;
        // 4. dispatch
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
        // browser
        fetch_with_request(req)
    }
}
