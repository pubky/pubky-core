//! Wasm specific implementation of methods used in the shared module
//!

use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

use futures_lite::{pin, Stream, StreamExt};

use pkarr::extra::endpoints::EndpointsResolver;
use pkarr::PublicKey;

use crate::{error::Result, Client};

impl Client {
    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url).expect("Invalid url in inner_request");

        if url.scheme() == "pubky" {
            url.set_scheme("https");
        }

        if PublicKey::try_from(original_url).is_ok() {
            let stream = self
                .pkarr
                .resolve_https_endpoints(url.host_str().unwrap_or(""));

            let mut so_far = None;

            while let Some(endpoint) = stream.next().await {
                if let Some(e) = so_far {
                    if e.domain() == "." && endpoint.domain() != "." {
                        so_far = Some(endpoint);
                    }
                } else {
                    so_far = Some(endpoint)
                }
            }

            if let Some(e) = so_far {
                url.set_host(Some(e.domain()));
                url.set_port(Some(e.port()));

                return self.http.request(method, url).fetch_credentials_include();
            } else {
                // TODO: didn't find any domain, what to do?
            }
        }

        self.http.request(method, url).fetch_credentials_include()
    }
}
