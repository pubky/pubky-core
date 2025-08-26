//! HTTP methods that support `https://` with Pkarr domains, and `pubky://` URLs

use crate::Client;
use pkarr::PublicKey;
use reqwest::{IntoUrl, Method, RequestBuilder};

impl Client {
    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [reqwest::Client::request], in that it can make requests to:
    /// 1. HTTPs URLs with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///    by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        let url = url.as_str();

        if url.starts_with("pubky://") {
            // Rewrite pubky:// urls to https://_pubky.
            let url = format!("https://_pubky.{}", url.split_at(8).1);

            return self.http.request(method, url);
            // PublicKey has methods to extract a publickey from a well-formed URL
        } else if url.starts_with("https://") && PublicKey::try_from(url).is_err() {
            // TODO: remove icann_http when we can control reqwest connection
            // and or create a tls config per connection.
            return self.icann_http.request(method, url);
        }

        self.http.request(method, url)
    }
}
