use reqwest::{Method, Response, header::COOKIE};
use url::Url;

use crate::{
    PubkyAgent, PublicKey, agent::state::sealed::Sealed, errors::Result, util::check_http_status,
};

/// Namespaced homeserver view: HTTP verbs + list, scoped to this agent.
// For now, given that the Homeserver has a custom API, we are going to call this namespace "Homeserver"
// In the future, when we stick with WebDav, we can rename to WebDav our add a new namespace "WebDav" if
// mantaining compatibility with old Homeservers
#[derive(Debug, Clone, Copy)]
pub struct Homeserver<'a, S: Sealed>(&'a PubkyAgent<S>);

impl<S: Sealed> PubkyAgent<S> {
    /// Entry point to homeserver-scoped verbs.
    pub fn homeserver(&self) -> Homeserver<'_, S> {
        Homeserver(self)
    }
}

impl<'a, S: Sealed> Homeserver<'a, S> {
    /// Base URL of this agent’s homeserver: `pubky://<pubky>/`.
    pub(crate) fn base_url(&self) -> Result<Url> {
        let pk = self.0.require_pubky()?;
        Url::parse(&format!("pubky://{}/", pk)).map_err(Into::into)
    }

    /// Build a request. If `path_or_url` is relative, target this agent’s homeserver.
    pub(crate) async fn request(
        &self,
        method: Method,
        path_or_url: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let url = match Url::parse(path_or_url) {
            Ok(abs) => abs,
            Err(_) => {
                let mut base = self.base_url()?;
                base.set_path(path_or_url);
                base
            }
        };

        let rb = self.0.client.cross_request(method, url.clone()).await?;

        // Attach session cookie only when the target host is this agent’s homeserver.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let matches_agent = self
                .0
                .pubky()
                .and_then(|pk| {
                    let host = url.host_str().unwrap_or("");
                    if let Some(tail) = host.strip_prefix("_pubky.") {
                        PublicKey::try_from(tail).ok().map(|h| h == pk)
                    } else {
                        PublicKey::try_from(host).ok().map(|h| h == pk)
                    }
                })
                .unwrap_or(false);

            if matches_agent {
                if let Ok(g) = self.0.session_secret.read() {
                    if let Some(secret) = g.as_ref() {
                        let cookie_name = self.0.require_pubky()?.to_string();
                        return Ok(rb.header(COOKIE, format!("{cookie_name}={secret}")));
                    }
                }
            }
        }

        Ok(rb)
    }

    /// Convenience: GET relative to this agent’s homeserver.
    pub async fn get(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.0.capture_session_cookie(&resp);
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
        let _ = self.0.capture_session_cookie(&resp);
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
        let _ = self.0.capture_session_cookie(&resp);
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
        let _ = self.0.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn delete(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.0.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn head(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.0.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    /// Directory listing helper (agent-scoped). Relative `path_or_url` is resolved to this agent.
    pub fn list(&self, path_or_url: &str) -> ListBuilder<'_, S> {
        ListBuilder {
            agent: self.0,
            path_or_url: path_or_url.to_string(),
            reverse: false,
            shallow: false,
            limit: None,
            cursor: None,
        }
    }
}

/// Homeserver-scoped List request builder.
/// Stores the original `path_or_url` and resolves at send() via the agent.
#[derive(Debug)]
pub struct ListBuilder<'a, S: Sealed> {
    agent: &'a PubkyAgent<S>,
    path_or_url: String,
    reverse: bool,
    shallow: bool,
    limit: Option<u16>,
    cursor: Option<String>,
}

impl<'a, S: Sealed> ListBuilder<'a, S> {
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }
    pub fn shallow(mut self, shallow: bool) -> Self {
        self.shallow = shallow;
        self
    }
    pub fn limit(mut self, limit: u16) -> Self {
        self.limit = Some(limit);
        self
    }
    pub fn cursor(mut self, cursor: &str) -> Self {
        self.cursor = Some(cursor.to_string());
        self
    }

    pub async fn send(self) -> Result<Vec<Url>> {
        // Resolve now (absolute stays absolute, relative is based on agent’s homeserver)
        let mut url = match Url::parse(&self.path_or_url) {
            Ok(abs) => abs,
            Err(_) => {
                let mut base = self.agent.homeserver().base_url()?;
                base.set_path(&self.path_or_url);
                base
            }
        };

        // ensure directory semantics (trailing slash)
        if !url.path().ends_with('/') {
            let path = url.path().to_string();
            let mut parts = path.split('/').collect::<Vec<_>>();
            parts.pop();
            let path = format!("{}/", parts.join("/"));
            url.set_path(&path);
        }

        {
            let mut q = url.query_pairs_mut();
            if self.reverse {
                q.append_key_only("reverse");
            }
            if self.shallow {
                q.append_key_only("shallow");
            }
            if let Some(limit) = self.limit {
                q.append_pair("limit", &limit.to_string());
            }
            if let Some(cursor) = self.cursor {
                q.append_pair("cursor", &cursor);
            }
        }

        // go through the agent to get proper cookie scoping
        let rb = self
            .agent
            .homeserver()
            .request(Method::GET, url.as_str())
            .await?;
        let resp = rb.send().await?;
        let resp = check_http_status(resp).await?;

        let bytes = resp.bytes().await?;
        let mut out = Vec::new();
        for line in String::from_utf8_lossy(&bytes).lines() {
            out.push(Url::parse(line)?);
        }
        Ok(out)
    }
}
