//! HTTP methods that support `https://` with Pkarr domains, and `pubky://` URLs

use pkarr::PublicKey;
use reqwest::{IntoUrl, Method, RequestBuilder};

use super::super::Client;

impl Client {
    #[cfg(not(wasm_browser))]
    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [reqwest::Client::request], in that it can make requests to:
    /// 1. HTTPs URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        let url = url.as_str();

        if url.starts_with("pubky://") {
            let url = format!("https://_pubky.{}", url.split_at(8).1);

            return self.http.request(method, url);
        } else if url.starts_with("https://") && PublicKey::try_from(url).is_err() {
            // TODO: remove icann_http when we can control reqwest connection
            // and or create a tls config per connection.
            return self.icann_http.request(method, url);
        }

        self.http.request(method, url)
    }

    /// Convenience method to make a `GET` request to a URL.
    ///
    /// Differs from [reqwest::Client::get], in that it can make requests to:
    /// 1. HTTP(s) URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::GET, url)
    }

    /// Convenience method to make a `POST` request to a URL.
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn post<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::POST, url)
    }

    /// Convenience method to make a `PUT` request to a URL.
    ///
    /// Differs from [reqwest::Client::put], in that it can make requests to:
    /// 1. HTTP(s) URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn put<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PUT, url)
    }

    /// Convenience method to make a `PATCH` request to a URL.
    ///
    /// Differs from [reqwest::Client::patch], in that it can make requests to:
    /// 1. HTTP(s) URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn patch<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::PATCH, url)
    }

    /// Convenience method to make a `DELETE` request to a URL.
    ///
    /// Differs from [reqwest::Client::delete], in that it can make requests to:
    /// 1. HTTP(s) URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn delete<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// Convenience method to make a `HEAD` request to a URL.
    ///
    /// Differs from [reqwest::Client::head], in that it can make requests to:
    /// 1. HTTP(s) URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn head<U: IntoUrl>(&self, url: U) -> RequestBuilder {
        self.request(Method::HEAD, url)
    }

    // === Private Methods ===

    pub(crate) async fn cross_request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        self.request(method, url)
    }
}

#[cfg(test)]
mod tests {
    use pubky_testnet::Testnet;

    #[tokio::test]
    async fn http_get_pubky() {
        let testnet = Testnet::run().await.unwrap();
        let homeserver = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let response = client
            .get(format!("https://{}/", homeserver.public_key()))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200)
    }

    #[tokio::test]
    async fn http_get_icann() {
        let testnet = Testnet::run().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let response = client
            .request(Default::default(), "https://example.com/")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }
}
