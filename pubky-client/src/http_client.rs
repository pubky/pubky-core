use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url, header::HeaderMap};

pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// Abstract interface for an HTTP client.
/// This allows swapping the backend between native `reqwest` and WASM `web_sys::fetch`.
// For native (multi-threaded) environments, require Send + Sync.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait HttpClient: Clone + Send + Sync + 'static {
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        headers: Option<HeaderMap>,
    ) -> Result<HttpResponse>;
}

// For WASM (single-threaded) environments, do NOT require Send or Sync.
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait HttpClient: Clone + 'static {
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        headers: Option<HeaderMap>,
    ) -> Result<HttpResponse>;
}
