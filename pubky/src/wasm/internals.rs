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

        self.transform_url(&mut url).await;

        self.http.request(method, url).fetch_credentials_include()
    }
}
