use reqwest::RequestBuilder;
use url::Url;

use crate::PubkyClient;

impl PubkyClient {
    // === HTTP ===

    /// A wrapper around [reqwest::Client::request], with the same signature between native and wasm.
    pub(crate) async fn inner_request(&self, method: reqwest::Method, url: Url) -> RequestBuilder {
        self.http.request(method, url)
    }
}
