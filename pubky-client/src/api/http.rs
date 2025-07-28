//! HTTP convenience methods for the generic Pubky client.
//! These methods handle `pubky://` scheme rewriting and delegate the actual request
//! to the underlying abstract `HttpClient`.

use anyhow::{Result, anyhow};
use reqwest::{Method, Url};

use crate::{Client, http::HttpClient};

impl<H: HttpClient> Client<H> {
    /// Performs a generic HTTP request after handling URL scheme specifics.
    ///
    /// This is the primary internal method that all convenience methods (get, post, etc.)
    /// delegate to. It parses the URL string, rewrites `pubky://` schemes to `https://_pubky.`,
    /// and then calls the abstract `HttpClient`.
    ///
    /// # Arguments
    /// * `method` - The HTTP method to use.
    /// * `url_str` - The URL to request, as a string slice.
    /// * `body` - An optional request body as bytes.
    ///
    /// # Returns
    /// A `Result` containing the response body as a `Vec<u8>`.
    pub async fn request(
        &self,
        method: Method,
        url_str: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        // Attempt to parse the URL.
        let mut url =
            Url::parse(url_str).map_err(|e| anyhow!("Invalid URL '{}': {}", url_str, e))?;

        // If the scheme is `pubky`, rewrite it to a format the HTTP client can understand.
        // e.g., "pubky://<key>/path" becomes "https://_pubky.<key>/path"
        if url.scheme() == "pubky" {
            if let Some(host_and_path) = url_str.strip_prefix("pubky://") {
                let rewritten_url = format!("https://_pubky.{}", host_and_path);
                url = Url::parse(&rewritten_url)?;
            }
        }

        // Delegate the actual request to the injected HttpClient implementation.
        self.http.request(method, url, body, None).await
    }

    /// Convenience method to make a `GET` request to a URL.
    pub async fn get(&self, url: &str) -> Result<Vec<u8>> {
        self.request(Method::GET, url, None).await
    }

    /// Convenience method to make a `POST` request to a URL with a body.
    pub async fn post(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>> {
        self.request(Method::POST, url, Some(body)).await
    }

    /// Convenience method to make a `PUT` request to a URL with a body.
    pub async fn put(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>> {
        self.request(Method::PUT, url, Some(body)).await
    }

    /// Convenience method to make a `PATCH` request to a URL with a body.
    pub async fn patch(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>> {
        self.request(Method::PATCH, url, Some(body)).await
    }

    /// Convenience method to make a `DELETE` request to a URL.
    pub async fn delete(&self, url: &str) -> Result<Vec<u8>> {
        self.request(Method::DELETE, url, None).await
    }

    /// Convenience method to make a `HEAD` request to a URL.
    pub async fn head(&self, url: &str) -> Result<Vec<u8>> {
        self.request(Method::HEAD, url, None).await
    }
}
