//! Wasm specific implementation of methods used in the shared module
//!

use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

use crate::Client;

impl Client {
    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url).expect("Invalid url in inner_request");

        let original_host = url.host_str().unwrap_or("").to_string();

        self.transform_url(&mut url).await;

        self.http
            .request(method, url)
            .header::<&str, &str>("pkarr-host", &original_host)
            .fetch_credentials_include()
    }
}
