use reqwest::{Method, Response};
use url::Url;

use crate::{
    Error, PubkyAgent, PubkyPath,
    agent::{path::IntoPubkyPath, state::sealed::Sealed},
    errors::Result,
    util::check_http_status,
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
    #[inline]
    fn to_url<P: IntoPubkyPath>(&self, p: P) -> Result<Url> {
        let addr: PubkyPath = p.into_pubky_path()?;
        let pubky_url = addr.to_pubky_url(Some(&self.0.require_pubky()?))?;
        let url = Url::parse(&pubky_url)?;
        Ok(url)
    }

    /// Build a request. If `path` is relative, target this agent’s homeserver.
    pub(crate) async fn request<P: IntoPubkyPath>(
        &self,
        method: Method,
        path: P,
    ) -> Result<reqwest::RequestBuilder> {
        let url = self.to_url(path)?;

        let rb = self.0.client.cross_request(method, url.clone()).await?;

        // Attach session cookie only when the target host is this agent’s homeserver.
        #[cfg(not(target_arch = "wasm32"))]
        let rb = self.0.maybe_attach_session_cookie(&url, rb)?;

        Ok(rb)
    }

    /// Convenience GET
    pub async fn get<P: IntoPubkyPath>(&self, path: P) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        check_http_status(resp).await
    }

    pub async fn put<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoPubkyPath,
        B: Into<reqwest::Body>,
    {
        let resp = self
            .request(Method::PUT, path)
            .await?
            .body(body)
            .send()
            .await?;
        check_http_status(resp).await
    }

    pub async fn post<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoPubkyPath,
        B: Into<reqwest::Body>,
    {
        let resp = self
            .request(Method::POST, path)
            .await?
            .body(body)
            .send()
            .await?;
        check_http_status(resp).await
    }

    pub async fn patch<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoPubkyPath,
        B: Into<reqwest::Body>,
    {
        let resp = self
            .request(Method::PATCH, path)
            .await?
            .body(body)
            .send()
            .await?;
        check_http_status(resp).await
    }

    pub async fn delete<P: IntoPubkyPath>(&self, path: P) -> Result<Response> {
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        check_http_status(resp).await
    }

    pub async fn head<P: IntoPubkyPath>(&self, path: P) -> Result<Response>
    where
        P: TryInto<PubkyPath, Error = Error>,
    {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        check_http_status(resp).await
    }

    /// Directory listing helper (agent-scoped). Relative `path` is resolved to this agent.
    pub fn list<P: IntoPubkyPath>(&self, path: P) -> ListBuilder<'_, S>
    where
        P: TryInto<PubkyPath, Error = Error>,
    {
        ListBuilder {
            agent: self.0,
            path: path.try_into().expect("validated by caller"),
            reverse: false,
            shallow: false,
            limit: None,
            cursor: None,
        }
    }
}

/// Homeserver-scoped List request builder.
#[derive(Debug)]
pub struct ListBuilder<'a, S: Sealed> {
    agent: &'a PubkyAgent<S>,
    path: PubkyPath,
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
        let mut url = Url::parse(&self.path.to_pubky_url(Some(&self.agent.require_pubky()?))?)?;

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
