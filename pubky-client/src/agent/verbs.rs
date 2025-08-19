use reqwest::{Method, Response, header::COOKIE};
use url::Url;

use crate::{PubkyAgent, PublicKey, errors::Result, util::check_http_status};

impl PubkyAgent {
    /// Build a request. If `path_or_url` is relative, targets this agent’s homeserver.
    pub async fn request(
        &self,
        method: Method,
        path_or_url: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let url = match Url::parse(path_or_url) {
            Ok(abs) => abs,
            Err(_) => {
                let mut base = self.homeserver_base()?;
                base.set_path(path_or_url);
                base
            }
        };

        let rb = self.client.cross_request(method, url.clone()).await?;

        // Attach session cookie only when the target host is this agent’s homeserver.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let matches_agent = self
                .pubky()
                .and_then(|pk| {
                    let host = url.host_str().unwrap_or("");
                    if host.starts_with("_pubky.") {
                        let tail = &host["_pubky.".len()..];
                        PublicKey::try_from(tail).ok().map(|h| h == pk)
                    } else {
                        PublicKey::try_from(host).ok().map(|h| h == pk)
                    }
                })
                .unwrap_or(false);

            if matches_agent {
                if let Ok(g) = self.session_secret.read() {
                    if let Some(secret) = g.as_ref() {
                        let cookie_name = self.require_pubky()?.to_string();
                        return Ok(rb.header(COOKIE, format!("{cookie_name}={secret}")));
                    }
                }
            }
        }

        Ok(rb)
    }

    pub async fn get(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn put<B: Into<reqwest::Body>>(&self, path: &str, body: B) -> Result<Response> {
        let resp = self
            .request(Method::PUT, path)
            .await?
            .body(body)
            .send()
            .await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn post<B: Into<reqwest::Body>>(&self, path: &str, body: B) -> Result<Response> {
        let resp = self
            .request(Method::POST, path)
            .await?
            .body(body)
            .send()
            .await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn patch<B: Into<reqwest::Body>>(&self, path: &str, body: B) -> Result<Response> {
        let resp = self
            .request(Method::PATCH, path)
            .await?
            .body(body)
            .send()
            .await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn delete(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn head(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }
}
