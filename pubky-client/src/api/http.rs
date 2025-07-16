//! HTTP methods that support `https://` with Pkarr domains, and `pubky://` URLs

use crate::Client;
use pkarr::PublicKey;
use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

#[cfg(not(target_arch = "wasm32"))]
impl Client {
    /// Start building a `Request` with the `Method` and `Url`.
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [reqwest::Client::request], in that it can make requests to:
    /// 1. HTTPs URLs with with a [pkarr::PublicKey] as Top Level Domain, by resolving
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
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///    by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
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
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///    by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
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
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///    by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
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
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///    by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
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
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. Pubky URLs like `pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///    by converting the url into `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
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

    pub async fn prepare_request(&self, _url: &mut Url) -> Option<String> {
        None
    }
}

#[cfg(target_arch = "wasm32")]
use futures_lite::StreamExt;
#[cfg(target_arch = "wasm32")]
use pkarr::extra::endpoints::Endpoint;

#[cfg(target_arch = "wasm32")]
impl Client {
    /// A wrapper around [NativeClient::request], with the same signature between native and wasm.
    pub(crate) async fn cross_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        let original_url = url.as_str();
        let mut url = Url::parse(original_url).expect("Invalid url in inner_request");

        if let Some(pubky_host) = self.prepare_request(&mut url).await {
            self.http
                .request(method, url.clone())
                .header::<&str, &str>("pubky-host", &pubky_host)
                .fetch_credentials_include()
        } else {
            self.http
                .request(method, url.clone())
                .fetch_credentials_include()
        }
    }

    /// - Transforms pubky:// url to http(s):// urls
    /// - Resolves a clearnet host to call with fetch
    /// - Returns the `pubky-host` value if available
    pub async fn prepare_request(&self, url: &mut Url) -> Option<String> {
        let host = url.host_str().unwrap_or("").to_string();

        if url.scheme() == "pubky" {
            *url = Url::parse(&format!("https{}", &url.as_str()[5..]))
                .expect("couldn't replace pubky:// with https://");
            url.set_host(Some(&format!("_pubky.{}", url.host_str().unwrap_or(""))))
                .expect("couldn't map pubk://<pubky> to https://_pubky.<pubky>");
        }

        let mut pubky_host = None;

        if PublicKey::try_from(host.clone()).is_ok() {
            self.transform_url(url).await;

            pubky_host = Some(host);
        };

        pubky_host
    }

    pub(crate) async fn transform_url(&self, url: &mut Url) {
        let clone = url.clone();
        let qname = clone.host_str().unwrap_or("");
        log::debug!("Prepare request {}", url.as_str());

        let mut stream = self.pkarr.resolve_https_endpoints(qname);

        let mut so_far: Option<Endpoint> = None;

        while let Some(endpoint) = stream.next().await {
            if endpoint.domain().is_some() {
                so_far = Some(endpoint);

                // TODO: currently we return the first thing we can see,
                // in the future we might want to failover to other endpoints
                break;
            }
        }

        if let Some(e) = so_far {
            // TODO: detect loopback IPs and other equivalent to localhost
            if e.domain() == Some("localhost") {
                url.set_scheme("http")
                    .expect("couldn't replace pubky:// with http://");

                let http_port = e
                    .get_param(pubky_common::constants::reserved_param_keys::HTTP_PORT)
                    .and_then(|x| <[u8; 2]>::try_from(x).ok())
                    .map(u16::from_be_bytes)
                    .expect("could not find HTTP_PORT service param");

                url.set_port(Some(http_port))
                    .expect("coultdn't use the resolved endpoint's port");
            } else if let Some(port) = e.port() {
                url.set_port(Some(port))
                    .expect("coultdn't use the resolved endpoint's port");
            }

            if let Some(domain) = e.domain() {
                url.set_host(Some(domain))
                    .expect("coultdn't use the resolved endpoint's domain");
            }

            log::debug!("Transformed URL to: {}", url.as_str());
        } else {
            // TODO: didn't find any domain, what to do?
            //  return an error.
            log::debug!("Could not resolve host: {}", qname);
        }
    }
}
