use crate::{PubkyHttpClient, PublicKey, Result, cross_log};
use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostKind {
    ResolvedPubky,
    Icann,
    Pubky,
}

fn classify_host(host: &str) -> HostKind {
    if let Some(pk_host) = host.strip_prefix("_pubky.") {
        if PublicKey::try_from(pk_host).is_ok() {
            return HostKind::ResolvedPubky;
        }
    } else if PublicKey::try_from(host).is_err() {
        return HostKind::Icann;
    }
    HostKind::Pubky
}

impl PubkyHttpClient {
    /// Constructs a [`reqwest::RequestBuilder`] for the given HTTP `method` and `url`,
    /// routing through the client’s unified request path.
    ///
    /// This method ensures that special Pubky and pkarr hosts are resolved according to
    /// platform-specific rules (native or WASM), including:
    /// - Detecting `_pubky.<public-key>` hosts and applying the correct TLS handling.
    /// - Routing standard ICANN domains through the `icann_http` client on native builds.
    ///
    /// On native targets, this is effectively a thin wrapper around [`PubkyHttpClient::request`],
    /// while on WASM it also performs host transformation and may add the `pubky-host` header.
    ///
    /// Returns a [`Result`] containing the prepared `RequestBuilder`, or a URL/transport
    /// parsing error if the supplied `url` is invalid.
    ///
    /// [`PubkyHttpClient::request`]: crate::PubkyHttpClient::request
    #[allow(
        clippy::unused_async,
        reason = "native implementation stays async to share the same signature as the WASM backend"
    )]
    pub(crate) async fn cross_request(&self, method: Method, url: Url) -> Result<RequestBuilder> {
        Ok(self.request(method, &url))
    }

    /// Start building a `Request` with the `Method` and `Url` (native-only)
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [`reqwest::Client::request`], in that it can make requests to:
    /// 1. HTTPS URLs with a [`crate::PublicKey`] as top-level domain, by resolving
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. `_pubky.<public-key>` URLs like `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: &U) -> RequestBuilder {
        let url_str = url.as_str();

        let host = Url::parse(url_str)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned));

        if let Some(ref host) = host {
            match classify_host(host) {
                HostKind::ResolvedPubky => {
                    cross_log!(
                        debug,
                        "Routing request for resolved _pubky host {} via Pubky TLS",
                        host
                    );
                    return self.http.request(method, url_str);
                }
                HostKind::Icann => {
                    // TODO: remove icann_http when we can control reqwest connection
                    // and or create a tls config per connection.
                    cross_log!(
                        debug,
                        "Routing request for ICANN host {} via standard TLS",
                        host
                    );
                    return self.icann_http.request(method, url_str);
                }
                HostKind::Pubky => {
                    cross_log!(
                        debug,
                        "Routing request for pubky host {} via PubkyTLS",
                        host
                    );
                }
            }
        }

        self.http.request(method, url_str)
    }
}
