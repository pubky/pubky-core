//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::Promise;
use wasm_bindgen::prelude::*;

use reqwest::Url;

use crate::PubkyClient;

#[wasm_bindgen]
impl PubkyClient {
    #[wasm_bindgen]
    pub async fn fetch(
        &self,
        url: &str,
        init: &web_sys::RequestInit,
    ) -> Result<js_sys::Promise, JsValue> {
        let mut url: Url = url.try_into().map_err(|err| {
            JsValue::from_str(&format!("PubkyClient::fetch(): Invalid `url`; {:?}", err))
        })?;

        self.resolve_url(&mut url).await.map_err(JsValue::from)?;

        let js_req =
            web_sys::Request::new_with_str_and_init(url.as_str(), init).map_err(|err| {
                JsValue::from_str(&format!("PubkyClient::fetch(): Invalid `init`; {:?}", err))
            })?;

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
            .unchecked_into::<web_sys::ServiceWorkerGlobalScope>()
            .fetch_with_request(req)
    } else {
        // browser
        fetch_with_request(req)
    }
}
