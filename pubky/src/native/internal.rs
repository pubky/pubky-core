//! Native specific implementation of methods used in the shared module
//!

use reqwest::{IntoUrl, Method, RequestBuilder};

use crate::Client;

impl Client {
    pub(crate) async fn inner_request<T: IntoUrl>(&self, method: Method, url: T) -> RequestBuilder {
        self.request(method, url)
    }
}
