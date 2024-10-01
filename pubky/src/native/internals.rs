use reqwest::RequestBuilder;
use url::Url;

use crate::PubkyClient;

use std::net::ToSocketAddrs;

use pkarr::{Client, EndpointResolver, PublicKey};
use reqwest::dns::{Addrs, Resolve};

pub struct PkarrResolver(Client);

impl Resolve for PkarrResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let client = self.0.clone();

        Box::pin(async move {
            let name = name.as_str();

            if PublicKey::try_from(name).is_ok() {
                let endpoint = client.resolve_endpoint(name).await?;

                let addrs: Addrs = Box::new(endpoint.to_socket_addrs().into_iter());
                return Ok(addrs);
            };

            Ok(Box::new(format!("{name}:0").to_socket_addrs().unwrap()))
        })
    }
}

impl From<&pkarr::Client> for PkarrResolver {
    fn from(pkarr: &pkarr::Client) -> Self {
        PkarrResolver(pkarr.clone())
    }
}

impl PubkyClient {
    // === HTTP ===

    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request(&self, method: reqwest::Method, url: Url) -> RequestBuilder {
        self.http.request(method, url)
    }
}
