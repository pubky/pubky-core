//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::Promise;
use wasm_bindgen::prelude::*;
use web_sys::{Headers, RequestInit};

use reqwest::{IntoUrl, Method, RequestBuilder, Url};

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
        request_init: Option<RequestInit>,
    ) -> Result<js_sys::Promise, JsValue> {
        let mut url: Url = url.try_into().map_err(|err| {
            JsValue::from_str(&format!("pubky::Client::fetch(): Invalid `url`; {:?}", err))
        })?;

        let request_init = request_init.unwrap_or_default();

        if let Some(pkarr_host) = self.prepare_request(&mut url).await {
            let headers = request_init.get_headers();

            let headers = if headers.is_null() || headers.is_undefined() {
                Headers::new()?
            } else {
                Headers::from(headers)
            };

            headers.append("pkarr-host", &pkarr_host)?;

            request_init.set_headers(&headers.into());
        }

        let js_req = web_sys::Request::new_with_str_and_init(url.as_str(), &request_init).map_err(
            |err| {
                JsValue::from_str(&format!(
                    "pubky::Client::fetch(): Invalid `init`; {:?}",
                    err
                ))
            },
        )?;

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

impl Client {
    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url).expect("Invalid url in inner_request");

        if let Some(pkarr_host) = self.prepare_request(&mut url).await {
            self.http
                .request(method, url.clone())
                .header::<&str, &str>("pkarr-host", &pkarr_host)
                .fetch_credentials_include()
        } else {
            self.http
                .request(method, url.clone())
                .fetch_credentials_include()
        }
    }

    /// - Transforms pubky:// url to http(s):// urls
    /// - Resolves a clearnet host to call with fetch
    /// - Returns the `pkarr-host` value if available
    pub(super) async fn prepare_request(&self, url: &mut Url) -> Option<String> {
        if url.scheme() == "pubky" {
            *url = Url::parse(&format!("https{}", &url.as_str()[5..]))
                .expect("couldn't replace pubky:// with https://");
            url.set_host(Some(&format!("_pubky.{}", url.host_str().unwrap_or(""))))
                .expect("couldn't map pubk://<pubky> to https://_pubky.<pubky>");
        }

        // TODO: move at the begining of the method.
        let host = url.host_str().unwrap_or("").to_string();

        let mut pkarr_host = None;

        if PublicKey::try_from(host.clone()).is_ok() {
            self.transform_url(url, &host).await;

            pkarr_host = Some(host);

            log::debug!("Transformed URL to: {}", url.as_str());
        };

        pkarr_host
    }

    pub async fn transform_url(&self, url: &mut Url, qname: &str) {
        let mut stream = self.pkarr.resolve_https_endpoints(qname);

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
}
