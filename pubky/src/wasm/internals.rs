//! Wasm specific implementation of methods used in the shared module
//!

use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

use futures_lite::StreamExt;

use pkarr::extra::endpoints::{Endpoint, EndpointsResolver};
use pkarr::PublicKey;

use crate::Client;

impl Client {
    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url).expect("Invalid url in inner_request");

        self.transform_url(&mut url).await;

        self.http.request(method, url).fetch_credentials_include()
    }

    pub(super) async fn transform_url(&self, url: &mut Url) {
        if url.scheme() == "pubky" {
            url.set_scheme("https")
                .expect("couldn't replace pubky:// with https://");
            url.set_host(Some(&format!("_pubky.{}", url.host_str().unwrap_or(""))))
                .expect("couldn't map pubk://<pubky> to https://_pubky.<pubky>");
        }

        let qname = url.host_str().unwrap_or("").to_string();

        // TODO: detect loopback IPs and other equivilants to localhost
        if qname == "localhost" && self.testnet {
            url.set_scheme("http")
                .expect("couldn't replace pubky:// with http://");
        }

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
