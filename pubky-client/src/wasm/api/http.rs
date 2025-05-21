//! Fetch method handling HTTP and Pubky urls with Pkarr TLD.

use js_sys::Promise;
use wasm_bindgen::prelude::*;
use web_sys::{Headers, RequestInit};

use reqwest::{IntoUrl, Method, RequestBuilder, Url};

use futures_lite::StreamExt;

use pkarr::extra::endpoints::Endpoint;
use pkarr::PublicKey;

use crate::wasm::js_result::JsResult;

use super::super::constructor::Client;

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen]
    pub async fn fetch(
        &self,
        url: &str,
        request_init: Option<RequestInit>,
    ) -> JsResult<js_sys::Promise> {
        let mut url: Url = url.try_into().map_err(|err| {
            JsValue::from_str(&format!("pubky::Client::fetch(): Invalid `url`; {:?}", err))
        })?;

        let request_init = request_init.unwrap_or_default();

        if let Some(pubky_host) = self.0.prepare_request(&mut url).await {
            let headers = request_init.get_headers();

            let headers = if headers.is_null() || headers.is_undefined() {
                Headers::new()?
            } else {
                Headers::from(headers)
            };

            headers.append("pubky-host", &pubky_host)?;

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

impl crate::NativeClient {
    /// A wrapper around [NativeClient::request], with the same signature between native and wasm.
    pub(crate) async fn cross_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url).expect("Invalid url in inner_request");

        if let Some(pubky_host) = self.prepare_request(&mut url).await {
            self.http
                .request(method, url.clone())
                .header::<&str, &str>("pubky-host", &pubky_host)
                .fetch_credentials_include()
        } else {
            self.http
                .request(method, url.clone())
                .fetch_credentials_include()
        }
    }

    /// - Transforms pubky:// url to http(s):// urls
    /// - Resolves a clearnet host to call with fetch
    /// - Returns the `pubky-host` value if available
    pub(crate) async fn prepare_request(&self, url: &mut Url) -> Option<String> {
        let host = url.host_str().unwrap_or("").to_string();

        if url.scheme() == "pubky" {
            *url = Url::parse(&format!("https{}", &url.as_str()[5..]))
                .expect("couldn't replace pubky:// with https://");
            url.set_host(Some(&format!("_pubky.{}", url.host_str().unwrap_or(""))))
                .expect("couldn't map pubk://<pubky> to https://_pubky.<pubky>");
        }

        let mut pubky_host = None;

        if PublicKey::try_from(host.clone()).is_ok() {
            self.transform_url(url).await;

            pubky_host = Some(host);
        };

        pubky_host
    }

    pub(crate) async fn transform_url(&self, url: &mut Url) {
        let clone = url.clone();
        let qname = clone.host_str().unwrap_or("");
        cross_debug!("Prepare request {}", url.as_str());

        let mut stream = self.pkarr.resolve_https_endpoints(qname);

        let mut so_far: Option<Endpoint> = None;

        while let Some(endpoint) = stream.next().await {
            if endpoint.domain().is_some() {
                so_far = Some(endpoint);

                // TODO: currently we return the first thing we can see,
                // in the future we might want to failover to other endpoints
                break;
            }
        }

        if let Some(e) = so_far {
            // TODO: detect loopback IPs and other equivalent to localhost
            if self.testnet && e.domain() == Some("localhost") {
                url.set_scheme("http")
                    .expect("couldn't replace pubky:// with http://");

                let http_port = e
                    .get_param(pubky_common::constants::reserved_param_keys::HTTP_PORT)
                    .and_then(|x| <[u8; 2]>::try_from(x).ok())
                    .map(u16::from_be_bytes)
                    .expect("could not find HTTP_PORT service param");

                url.set_port(Some(http_port))
                    .expect("coultdn't use the resolved endpoint's port");
            } else if let Some(port) = e.port() {
                url.set_port(Some(port))
                    .expect("coultdn't use the resolved endpoint's port");
            }

            if let Some(domain) = e.domain() {
                url.set_host(Some(domain))
                    .expect("coultdn't use the resolved endpoint's domain");
            }

            log::debug!("Transformed URL to: {}", url.as_str());
        } else {
            // TODO: didn't find any domain, what to do?
            //  return an error.
            cross_debug!("Could not resolve host: {}", qname);
        }
    }
}
