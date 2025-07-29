//! WASM-specific implementation of the `HttpClient` trait using the browser's `fetch` API.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_lite::StreamExt;
use pkarr::PublicKey;
use pubky::http_client::{HttpClient, HttpResponse};
use reqwest::{
    Method, StatusCode, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use wasm_bindgen::{JsCast, JsValue}; // FIX: Import the JsCast trait to use .dyn_into()
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

/// The WASM implementation of `HttpClient` using `web_sys::fetch`.
///
/// This struct holds the necessary state to perform Pkarr resolution and URL
/// transformation before making a request in a web environment.
#[derive(Clone, Debug)]
pub struct WasmHttpClient {
    /// The pkarr client used to resolve Pkarr domains before making HTTP requests.
    pkarr_client: pkarr::Client,
    /// An optional hostname used to identify and correctly handle requests to a testnet.
    pub testnet_host: Option<String>,
}

impl WasmHttpClient {
    /// Creates a new `WasmHttpClient`.
    pub fn new(pkarr_client: pkarr::Client, testnet_host: Option<String>) -> Self {
        Self {
            pkarr_client,
            testnet_host,
        }
    }

    /// Prepares a URL and headers for a web request.
    ///
    /// This method resolves Pkarr domains to clearnet hosts, handles testnet
    /// transformations, and adds the `pubky-host` header if necessary.
    async fn prepare_request_for_fetch(
        &self,
        url: &mut Url,
    ) -> Result<Option<(HeaderName, String)>> {
        let original_host = url.host_str().unwrap_or("").to_string();

        // Only perform resolution for domains that are valid Pkarr public keys.
        if PublicKey::try_from(original_host.as_str()).is_err() {
            return Ok(None);
        }

        // --- Pkarr Resolution Logic ---
        let endpoint = self
            .pkarr_client
            .resolve_https_endpoints(&original_host)
            .find(|ep| ep.domain().is_some())
            .await;

        if let Some(ep) = endpoint {
            let is_testnet = ep.domain().map_or(false, |d| {
                d == "localhost" || self.testnet_host.as_deref() == Some(d)
            });

            if is_testnet {
                url.set_scheme("http")
                    .map_err(|_| anyhow!("Failed to set scheme to http for testnet"))?;
                let http_port = ep
                    .get_param(pubky_common::constants::reserved_param_keys::HTTP_PORT)
                    .and_then(|x| <[u8; 2]>::try_from(x).ok())
                    .map(u16::from_be_bytes)
                    .ok_or_else(|| anyhow!("Testnet endpoint missing HTTP_PORT param"))?;
                url.set_port(Some(http_port))
                    .map_err(|_| anyhow!("Failed to set testnet port"))?;
            } else if let Some(port) = ep.port() {
                url.set_port(Some(port))
                    .map_err(|_| anyhow!("Failed to set port"))?;
            }

            if let Some(domain) = ep.domain() {
                url.set_host(Some(domain))
                    .map_err(|_| anyhow!("Failed to set host"))?;
            }
        } else {
            return Err(anyhow!("Could not resolve Pkarr domain: {}", original_host));
        }

        Ok(Some((HeaderName::from_static("pubky-host"), original_host)))
    }
}

/// Converts a `web_sys::Response` into a `pubky::http_client::HttpResponse`.
async fn http_response_from_web_sys(resp: &Response) -> Result<HttpResponse> {
    // 1. Get status code
    let status = StatusCode::from_u16(resp.status())?;

    // 2. Get headers
    let mut headers = HeaderMap::new();
    let js_resp_headers = resp.headers();
    let iterator = js_sys::try_iter(&js_resp_headers)
        .map_err(|_| anyhow!("Response headers are not iterable"))?
        .ok_or_else(|| anyhow!("Could not create headers iterator"))?;

    for item in iterator {
        let item = item.map_err(|_| anyhow!("Error iterating headers"))?;
        let arr: js_sys::Array = item.dyn_into().unwrap();
        let key = arr.get(0).as_string().unwrap();
        let value = arr.get(1).as_string().unwrap();

        if let (Ok(h_name), Ok(h_value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            headers.append(h_name, h_value);
        }
    }

    // 3. Get body
    let promise = resp
        .array_buffer()
        .map_err(|e| anyhow!("Failed to get array buffer promise: {:?}", e))?;
    let buffer_value = JsFuture::from(promise)
        .await
        .map_err(|e| anyhow!("Failed to await array buffer: {:?}", e))?;
    let body = js_sys::Uint8Array::new(&buffer_value).to_vec();

    // 4. Return the final struct
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

#[async_trait(?Send)]
impl HttpClient for WasmHttpClient {
    async fn request(
        &self,
        method: Method,
        mut url: Url,
        body: Option<Vec<u8>>,
        headers: Option<HeaderMap>,
    ) -> Result<HttpResponse> {
        // 1. Prepare URL and get the special `pubky-host` header if needed.
        let pubky_host_header = self.prepare_request_for_fetch(&mut url).await?;

        // 2. Build the web_sys::Request.
        let opts = RequestInit::new();
        opts.set_method(method.as_str());
        opts.set_mode(RequestMode::Cors);

        if let Some(body_bytes) = body {
            // Create the Uint8Array and convert it into a JsValue.
            let js_body: JsValue = js_sys::Uint8Array::from(&body_bytes[..]).into();
            // Pass a reference to the JsValue.
            opts.set_body(&js_body);
        }

        let js_headers =
            web_sys::Headers::new().map_err(|e| anyhow!("Failed to create headers: {:?}", e))?;
        if let Some(header_map) = headers {
            for (name, value) in header_map.iter() {
                js_headers
                    .append(name.as_str(), value.to_str()?)
                    .map_err(|e| anyhow!("Failed to append header: {:?}", e))?;
            }
        }
        if let Some((name, value)) = pubky_host_header {
            js_headers
                .append(name.as_str(), &value)
                .map_err(|e| anyhow!("Failed to append pubky-host header: {:?}", e))?;
        }
        opts.set_headers(&js_headers);

        let request = Request::new_with_str_and_init(url.as_str(), &opts)
            .map_err(|e| anyhow!("Failed to create request: {:?}", e))?;

        // 3. Make the fetch call.
        let window = web_sys::window().ok_or_else(|| anyhow!("Could not get window object"))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| anyhow!("Fetch promise failed: {:?}", e))?;
        let resp: Response = resp_value
            .dyn_into()
            .map_err(|_| anyhow!("Could not cast JsValue to Response"))?;

        // 4. Check for HTTP errors.
        if !resp.ok() {
            return Err(anyhow!(
                "Fetch error: {} {}",
                resp.status(),
                resp.status_text()
            ));
        }

        http_response_from_web_sys(&resp).await
    }
}
