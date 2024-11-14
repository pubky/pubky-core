use reqwest::{IntoUrl, Method, RequestBuilder};

use crate::Client;

impl Client {
    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [reqwest::Client::request], in that it can make requests to:
    /// 1. HTTP(s) URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
    ///     corresponding endpoints, and verifying TLS certificates accordingly.
    ///     (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///     by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: U) -> RequestBuilder {
        self.http.request(method, url)
    }
}

#[cfg(test)]
mod tests {
    use pkarr::mainline::Testnet;
    use pubky_homeserver::Homeserver;

    use crate::Client;

    #[tokio::test]
    async fn http_get_pubky() {
        let testnet = Testnet::new(10).unwrap();

        let homeserver = Homeserver::start_test(&testnet).await.unwrap();

        let client = Client::test(&testnet);

        let url = format!("http://{}/", homeserver.public_key());

        let response = client
            .request(Default::default(), url)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200)
    }

    #[tokio::test]
    async fn http_get_icann() {
        let testnet = Testnet::new(10).unwrap();

        let client = Client::test(&testnet);

        let url = format!("http://example.com/");

        let response = client
            .request(Default::default(), url)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }
}
