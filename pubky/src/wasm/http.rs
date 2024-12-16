//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::Promise;
use wasm_bindgen::prelude::*;

use reqwest::Url;

use futures_lite::StreamExt;

use pkarr::extra::endpoints::{Endpoint, EndpointsResolver};
use pkarr::PublicKey;

use crate::Client;

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen]
    pub async fn fetch(
        &self,
        url: &str,
        init: &web_sys::RequestInit,
    ) -> Result<js_sys::Promise, JsValue> {
        let mut url: Url = url.try_into().map_err(|err| {
            JsValue::from_str(&format!("pubky::Client::fetch(): Invalid `url`; {:?}", err))
        })?;

        self.transform_url(&mut url).await;

        let js_req =
            web_sys::Request::new_with_str_and_init(url.as_str(), init).map_err(|err| {
                JsValue::from_str(&format!(
                    "pubky::Client::fetch(): Invalid `init`; {:?}",
                    err
                ))
            })?;

        Ok(js_fetch(&js_req))
    }

    pub(super) async fn transform_url(&self, url: &mut Url) {
        if url.scheme() == "pubky" {
            *url = Url::parse(&format!("https{}", &url.as_str()[5..]))
                .expect("couldn't replace pubky:// with https://");
            url.set_host(Some(&format!("_pubky.{}", url.host_str().unwrap_or(""))))
                .expect("couldn't map pubk://<pubky> to https://_pubky.<pubky>");
        }

        let qname = url.host_str().unwrap_or("").to_string();

        if PublicKey::try_from(qname.to_string()).is_ok() {
            let mut stream = self.pkarr.resolve_https_endpoints(&qname);

            let mut so_far: Option<Endpoint> = None;

            // TODO: currently we return the first thing we can see,
            // in the future we might want to failover to other endpoints
            while so_far.is_none() {
                while let Some(endpoint) = stream.next().await {
                    if endpoint.domain() != "." {
                        so_far = Some(endpoint);
                    }
                }
            }

            if let Some(e) = so_far {
                // TODO: detect loopback IPs and other equivilants to localhost
                if self.testnet && e.domain() == "localhost" {
                    url.set_scheme("http")
                        .expect("couldn't replace pubky:// with http://");
                }

                url.set_host(Some(e.domain()))
                    .expect("coultdn't use the resolved endpoint's domain");
                url.set_port(Some(e.port()))
                    .expect("coultdn't use the resolved endpoint's port");
            } else {
                // TODO: didn't find any domain, what to do?
            }
        }

        log::debug!("Transformed URL to: {}", url.as_str());
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
