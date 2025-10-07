//! HTTP methods that support `https://` with Pkarr domains, including `_pubky.<pk>` URLs

use crate::PubkyHttpClient;
use crate::errors::{PkarrError, Result};
use futures_lite::StreamExt;
use pkarr::PublicKey;
use pkarr::extra::endpoints::Endpoint;
use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

impl PubkyHttpClient {
    /// A wrapper around [PubkyHttpClient::request], with the same signature between native and wasm.
    pub(crate) async fn cross_request<T: IntoUrl>(
        &self,
        method: Method,
        url: T,
    ) -> Result<RequestBuilder> {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url)?;

        let pubky_host = self.prepare_request(&mut url).await?;

        let builder = self
            .http
            .request(method, url.clone())
            .fetch_credentials_include();

        let builder = if let Some(pubky_host) = pubky_host {
            builder.header("pubky-host", pubky_host)
        } else {
            builder
        };

        Ok(builder)
    }

    /// - Resolves a clearnet host to call with fetch
    /// - Returns the `pubky-host` value if available
    pub async fn prepare_request(&self, url: &mut Url) -> Result<Option<String>> {
        let host = url.host_str().unwrap_or("").to_string();

        let mut pubky_host = None;

        if let Some(stripped) = host.strip_prefix("_pubky.") {
            if PublicKey::try_from(stripped).is_ok() {
                self.transform_url(url).await?;
                pubky_host = Some(stripped.to_string());
            }
        } else if PublicKey::try_from(host.clone()).is_ok() {
            self.transform_url(url).await?;
            pubky_host = Some(host);
        };

        Ok(pubky_host)
    }

    async fn transform_url(&self, url: &mut Url) -> Result<()> {
        let clone = url.clone();
        let qname = clone.host_str().unwrap_or("").to_string();
        log::debug!("Prepare request {}", url.as_str());

        let stream = self.pkarr.resolve_https_endpoints(qname);

        self.transform_url_with_stream(url, qname, stream).await
    }

    async fn transform_url_with_stream<S>(
        &self,
        url: &mut Url,
        qname: &str,
        mut stream: S,
    ) -> Result<()>
    where
        S: futures_lite::Stream<Item = Endpoint> + Unpin,
    {
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
            // Check if the resolved domain is a testnet domain. It is if it's "localhost"
            // or if it matches the testnet_host configured in the client.
            let is_testnet_domain = e.domain().map_or(false, |domain| {
                if domain == "localhost" {
                    return true;
                }
                if let Some(test_host) = &self.testnet_host {
                    return domain == test_host;
                }
                false
            });

            // TODO: detect loopback IPs and other equivalent to localhost
            if is_testnet_domain {
                url.set_scheme("http")
                    .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;

                let http_port = e
                    .get_param(pubky_common::constants::reserved_param_keys::HTTP_PORT)
                    .and_then(|x| <[u8; 2]>::try_from(x).ok())
                    .map(u16::from_be_bytes)
                    .ok_or_else(|| {
                        PkarrError::InvalidRecord(
                            "could not find HTTP_PORT service param in Pkarr record".to_string(),
                        )
                    })?;

                url.set_port(Some(http_port))
                    .map_err(|_| url::ParseError::InvalidPort)?;
            } else if let Some(port) = e.port() {
                url.set_port(Some(port))
                    .map_err(|_| url::ParseError::InvalidPort)?;
            }

            if let Some(domain) = e.domain() {
                url.set_host(Some(domain))
                    .map_err(|_| url::ParseError::SetHostOnCannotBeABaseUrl)?;
            }

            log::debug!("Transformed URL to: {}", url.as_str());
        } else {
            // TODO: didn't find any domain, what to do?
            //  return an error.
            log::debug!("Could not resolve host: {}", qname);
            let host_display = if qname.is_empty() {
                "<empty host>".to_string()
            } else {
                qname.clone()
            };
            return Err(PkarrError::InvalidRecord(format!(
                "No HTTPS endpoints found in PKARR record for `{host}` (original URL: {url})",
                host = host_display,
                url = clone.as_str()
            ))
            .into());
        }

        Ok(())
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use futures_lite::stream;
    use pkarr::Keypair;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test(async)]
    async fn transform_url_errors_when_no_domain_is_found() {
        let client = PubkyHttpClient::new().unwrap();
        let pk = Keypair::random().public_key().to_string();
        let mut url = Url::parse(&format!("https://_pubky.{pk}/pub/app/file.txt")).unwrap();

        let result = client
            .transform_url_with_stream(&mut url, url.host_str().unwrap(), stream::empty())
            .await;

        let err = result.expect_err("transform_url should fail when no endpoint is resolved");

        let crate::errors::Error::Pkarr(PkarrError::InvalidRecord(message)) = err else {
            panic!("expected pkarr invalid record error, got {err:?}");
        };

        assert!(message.contains("could not resolve domain endpoint"));
    }
}
