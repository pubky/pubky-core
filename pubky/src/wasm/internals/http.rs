use crate::PubkyClient;

use reqwest::{Method, RequestBuilder, Response};
use url::Url;

impl PubkyClient {
    pub(crate) fn request(&self, method: Method, url: Url) -> RequestBuilder {
        let mut request = self.http.request(method, url).fetch_credentials_include();

        for cookie in self.session_cookies.read().unwrap().iter() {
            request = request.header("Cookie", cookie);
        }

        request
    }

    // Support cookies for nodejs

    pub(crate) fn store_session(&self, response: &Response) {
        if let Some(cookie) = response
            .headers()
            .get("set-cookie")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(';').next())
        {
            self.session_cookies
                .write()
                .unwrap()
                .insert(cookie.to_string());
        }
    }
    pub(crate) fn remove_session(&self, pubky: &pkarr::PublicKey) {
        let key = pubky.to_string();

        self.session_cookies
            .write()
            .unwrap()
            .retain(|cookie| !cookie.starts_with(&key));
    }
}
