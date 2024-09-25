use reqwest::{IntoUrl, Method, RequestBuilder};

use crate::PubkyClient;

impl PubkyClient {
    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [reqwest::Client::request], in that it can make requests
    /// to URLs with a [pkarr::PublicKey] as Top Level Domain, by resolving
    /// corresponding endpoints, and verifying TLS certificates accordingly.
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

    use crate::*;

    #[tokio::test]
    async fn http_get_pubky() {
        let testnet = Testnet::new(10);

        let homeserver = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::builder().testnet(&testnet).build();

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
        let testnet = Testnet::new(10);

        let client = PubkyClient::builder().testnet(&testnet).build();

        let url = format!("http://example.com/");

        let response = client
            .request(Default::default(), url)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }
}
