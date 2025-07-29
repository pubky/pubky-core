use crate::{http_client::HttpClient, internal::cookies::CookieJar};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Method, Url, header::HeaderMap};
use std::sync::Arc;

/// The native Reqwest-based implementation of `HttpClient`.
/// It internally manages two `reqwest` clients: one for Pkarr domains (with custom TLS)
/// and one for standard ICANN domains.
#[derive(Clone, Debug)]
pub struct NativeHttpClient {
    pub pkarr_client: reqwest::Client,
    pub icann_client: reqwest::Client,
    /// The cookie store, made public to allow explicit session deletion.
    pub cookie_store: Arc<CookieJar>,
}

#[async_trait]
impl HttpClient for NativeHttpClient {
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        headers: Option<HeaderMap>,
    ) -> Result<Vec<u8>> {
        // Determine if the URL is for a Pkarr domain to select the correct client.
        let is_pkarr_domain = url
            .host_str()
            .and_then(|h| pkarr::PublicKey::try_from(h).ok())
            .is_some();

        let client_to_use = if is_pkarr_domain {
            &self.pkarr_client
        } else {
            &self.icann_client
        };

        let mut request_builder = client_to_use.request(method, url);

        if let Some(body_bytes) = body {
            request_builder = request_builder.body(body_bytes);
        }
        if let Some(header_map) = headers {
            request_builder = request_builder.headers(header_map);
        }

        let response = request_builder.send().await?;

        // Ensure the request was successful before processing the body.
        let successful_response = response.error_for_status()?;

        // Return the response body as a vector of bytes.
        Ok(successful_response.bytes().await?.to_vec())
    }
}
