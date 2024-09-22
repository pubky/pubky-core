use crate::PubkyClient;

use reqwest::{Method, RequestBuilder};
use url::Url;

impl PubkyClient {
    pub(crate) fn inner_request(&self, method: Method, url: Url) -> RequestBuilder {
        let mut request = self.http.request(method, url).fetch_credentials_include();

        for cookie in self.session_cookies.read().unwrap().iter() {
            request = request.header("Cookie", cookie);
        }

        request
    }
}
