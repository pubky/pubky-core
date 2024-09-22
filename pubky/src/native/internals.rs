use reqwest::RequestBuilder;
use url::Url;

use crate::PubkyClient;

mod endpoints;
pub mod resolver;

impl PubkyClient {
    // === HTTP ===

    pub(crate) fn inner_request(&self, method: reqwest::Method, url: Url) -> RequestBuilder {
        self.http.request(method, url)
    }
}
