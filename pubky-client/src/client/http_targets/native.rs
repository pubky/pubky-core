use crate::{PubkyClient, Result};
use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

impl PubkyClient {
    /// No-op. Unused. This `pub` function exists only to avoid a clippy error on the `binding/js` crate.
    /// TODO: find a better solution to this.
    pub async fn prepare_request(&self, _url: &mut Url) -> Result<Option<String>> {
        Ok(None)
    }

    /// Constructs a [`reqwest::RequestBuilder`] for the given HTTP `method` and `url`,
    /// routing through the client’s unified request path.
    ///
    /// This method ensures that special Pubky and pkarr URL schemes or hosts are
    /// normalized and resolved according to platform-specific rules (native or WASM),
    /// including:
    /// - Translating `pubky://` URLs into the appropriate HTTPS form.
    /// - Detecting pkarr public key hostnames and applying the correct resolution/TLS handling.
    /// - Routing standard ICANN domains through the `icann_http` client on native builds.
    ///
    /// On native targets, this is effectively a thin wrapper around [`PubkyClient::request`],
    /// while on WASM it also performs host transformation and may add the `pubky-host` header.
    ///
    /// Returns a [`Result`] containing the prepared `RequestBuilder`, or a URL/transport
    /// parsing error if the supplied `url` is invalid.
    ///
    /// [`PubkyClient::request`]: crate::PubkyClient::request
    pub(crate) async fn cross_request<U: IntoUrl>(
        &self,
        method: Method,
        url: U,
    ) -> Result<RequestBuilder> {
        Ok(self.request(method, url))
    }
}
