use reqwest::{Method, RequestBuilder};
use url::Url;

use pkarr::{EndpointResolver, PublicKey};

use crate::{error::Result, PubkyClient};

// TODO: remove expect
pub async fn resolve(pkarr: &pkarr::Client, url: &mut Url) -> Result<()> {
    let qname = url.host_str().expect("URL TO HAVE A HOST!").to_string();

    // If http and has a Pubky TLD, switch to socket addresses.
    if url.scheme() == "http" && PublicKey::try_from(qname.as_str()).is_ok() {
        let endpoint = pkarr.resolve_endpoint(&qname).await?;

        if let Some(socket_address) = endpoint.to_socket_addrs().into_iter().next() {
            url.set_host(Some(&socket_address.to_string()))?;
            let _ = url.set_port(Some(socket_address.port()));
        } else if let Some(port) = endpoint.port() {
            url.set_host(Some(endpoint.target()))?;
            let _ = url.set_port(Some(port));
        }
    };

    Ok(())
}

impl PubkyClient {
    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request(&self, method: Method, url: Url) -> RequestBuilder {
        self.http.request(method, url).fetch_credentials_include()
    }
}
