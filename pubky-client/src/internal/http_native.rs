use crate::{Client, Result};
use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

impl Client {
    // Unused. Exists only to avoid a clippy error on the `binding/js` crate.
    pub async fn prepare_request(&self, _url: &mut Url) -> Result<Option<String>> {
        Ok(None)
    }

    // === Private Methods ===

    pub(crate) async fn cross_request<U: IntoUrl>(
        &self,
        method: Method,
        url: U,
    ) -> Result<RequestBuilder> {
        Ok(self.request(method, url))
    }
}
