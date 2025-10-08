//! Fetch method handling HTTP(S) URLs with Pkarr TLD support.

use js_sys::{Promise, Reflect};
use url::Url;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Headers, Request, RequestCredentials, RequestInit, Response, ServiceWorkerGlobalScope,
};

use super::constructor::Client;
use crate::js_error::{JsResult, PubkyError, PubkyErrorName};

#[wasm_bindgen]
impl Client {
    /// Perform a raw fetch. Works with `http(s)://` URLs.
    ///
    /// @param {string} url
    /// @param {RequestInit} init Standard fetch options; `credentials: "include"` recommended for session I/O.
    /// @returns {Promise<Response>}
    ///
    /// @example
    /// const client = pubky.client;
    /// const res = await client.fetch(`https://_pubky.${user}/pub/app/file.txt`, { method: "PUT", body: "hi", credentials: "include" });
    pub async fn fetch(&self, url: &str, init: Option<RequestInitArg>) -> JsResult<Response> {
        // 1) Parse URL
        let url = Url::parse(url)?;

        if url.scheme() == "pubky" {
            return Err(PubkyError::new(
                PubkyErrorName::InvalidInput,
                "pubky:// URLs are not supported; resolve them before transport",
            ));
        }

        // 2) Ask the SDK to prepare (resolve pkarr, adjust host, etc.)
        //    Returns Some(<z32>) iff this targets a Pubky host.
        #[cfg(target_arch = "wasm32")]
        let pubky_host = self.0.prepare_request(&mut url).await?;

        #[cfg(not(target_arch = "wasm32"))]
        let pubky_host: Option<String> = None;

        // 3) Start from caller's init; DO NOT clobber headers.
        let req_init = init
            .map(|init| RequestInit::from(JsValue::from(init)))
            .unwrap_or_default();

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
        let promise = js_fetch(&js_req);
        let value = JsFuture::from(promise).await.map_err(map_fetch_error)?;
        value.dyn_into::<Response>().map_err(PubkyError::from)
    }
}

fn map_fetch_error(err: JsValue) -> PubkyError {
    if err.is_instance_of::<js_sys::Error>() {
        let js_err: js_sys::Error = err.unchecked_into();
        let message = js_err
            .to_string()
            .as_string()
            .unwrap_or_else(|| "fetch failed".to_string());
        return PubkyError::new(PubkyErrorName::RequestError, message);
    }

    let message = err
        .as_string()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "fetch failed".to_string());

    PubkyError::new(PubkyErrorName::RequestError, message)
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "RequestInit")]
    pub type RequestInitArg;

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
    use pkarr::{CacheKey, Keypair, SignedPacket};
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    // Missing PKARR endpoints must surface a descriptive error so callers can react.
    #[wasm_bindgen_test(async)]
    async fn prepare_missing_endpoint_returns_error() {
        let client = Client::testnet(None).unwrap();
        let keypair = Keypair::random();
        seed_pkarr_testnet_endpoint(&client, &keypair, "localhost", 15411);
        let pk = keypair.public_key().to_string();
        let mut url = Url::parse(&format!("https://_pubky.{}/pub/file.txt", pk)).unwrap();

        let err = client.0.prepare_request(&mut url).await.unwrap_err();
        match err {
            pubky::errors::Error::Pkarr(pubky::errors::PkarrError::InvalidRecord(message)) => {
                assert!(message.contains("No HTTPS endpoints"), "message: {message}");
                assert!(
                    message.contains(&pk),
                    "error message should reference the requested public key: {message}"
                );
            }
            other => panic!("expected InvalidRecord error, got {other:?}"),
        }
    }

    // ICANN URL must not require pubky-host but should still allow credentials=include
    #[wasm_bindgen_test(async)]
    async fn prepare_icann_does_not_set_pubky_host() {
        let client = Client::new(None).unwrap();
        let mut url = Url::parse("https://example.com/").unwrap();

        let host_opt = client.0.prepare_request(&mut url).await.unwrap();
        assert!(host_opt.is_none());
    }

    fn seed_pkarr_testnet_endpoint(client: &Client, keypair: &Keypair, _host: &str, _port: u16) {
        let pkarr_client = client.0.pkarr();
        let cache = pkarr_client
            .cache()
            .expect("pkarr client should expose a cache for tests");

        let cache_key: CacheKey = keypair.public_key().into();
        let signed_packet = SignedPacket::builder()
            .sign(keypair)
            .expect("sign empty packet");

        cache.put(&cache_key, &signed_packet);
    }
}
