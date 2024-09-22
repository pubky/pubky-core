use crate::PubkyClient;

use reqwest::{Method, RequestBuilder};
use url::Url;

impl PubkyClient {
    pub(crate) fn inner_request(&self, method: Method, url: Url) -> RequestBuilder {
        self.http.request(method, url).fetch_credentials_include()
    }
}
