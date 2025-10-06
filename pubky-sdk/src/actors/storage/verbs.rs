use reqwest::{Method, Response, StatusCode};

use super::core::{PublicStorage, SessionStorage};
use super::resource::{IntoPubkyResource, IntoResourcePath};
use super::stats::ResourceStats;
use crate::Result;
use crate::util::check_http_status;

//
// SessionStorage (authenticated, as-me)
//

impl SessionStorage {
    /// HTTP `GET` (as me) for an **absolute path**.
    ///
    /// # Example
    /// ```no_run
    /// # async fn ex(session: pubky::PubkySession) -> pubky::Result<()> {
    /// let text = session
    ///     .storage()
    ///     .get("/pub/my.app/hello.txt").await?
    ///     .text().await?;
    /// # Ok(()) }
    /// ```
    pub async fn get<P: IntoResourcePath>(&self, path: P) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        check_http_status(resp).await
    }

    /// Lightweight existence check (HEAD) for an **absolute path**.
    pub async fn exists<P: IntoResourcePath>(&self, path: P) -> Result<bool> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        match resp.status() {
            s if s.is_success() => Ok(true),
            StatusCode::NOT_FOUND | StatusCode::GONE => Ok(false),
            _ => {
                let _ = check_http_status(resp).await?;
                Ok(false)
            }
        }
    }

    /// Retrieve metadata via `HEAD` for an **absolute path** (no body).
    pub async fn stats<P: IntoResourcePath>(&self, path: P) -> Result<Option<ResourceStats>> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        if resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::GONE {
            return Ok(None);
        }
        let resp = check_http_status(resp).await?;
        Ok(Some(ResourceStats::from_headers(resp.headers())))
    }

    /// HTTP `PUT` (write) for an **absolute path**.
    ///
    /// Requires a valid session; this handle is authenticated already.
    pub async fn put<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoResourcePath,
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

    /// HTTP `DELETE` for an **absolute path**.
    pub async fn delete<P: IntoResourcePath>(&self, path: P) -> Result<Response> {
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        check_http_status(resp).await
    }
}

//
// PublicStorage (unauthenticated, any user)
//

impl PublicStorage {
    /// HTTP `GET` for an **addressed resource** (`pubky<pk>/<abs-path>`, `<pk>/<abs-path>`, or `pubky://…`).
    ///
    /// # Example
    /// ```no_run
    /// # async fn ex() -> pubky::Result<()> {
    /// let storage = pubky::PublicStorage::new()?;
    /// let resp = storage.get("{other_pk}/pub/my.app/file.txt").await?;
    /// let bytes = resp.bytes().await?;
    /// # Ok(()) }
    /// ```
    pub async fn get<A: IntoPubkyResource>(&self, addr: A) -> Result<Response> {
        let resp = self.request(Method::GET, addr).await?.send().await?;
        check_http_status(resp).await
    }

    /// HEAD existence check for an addressed resource.
    pub async fn exists<A: IntoPubkyResource>(&self, addr: A) -> Result<bool> {
        let resp = self.request(Method::HEAD, addr).await?.send().await?;
        match resp.status() {
            s if s.is_success() => Ok(true),
            StatusCode::NOT_FOUND | StatusCode::GONE => Ok(false),
            _ => {
                let _ = check_http_status(resp).await?;
                Ok(false)
            }
        }
    }

    /// Metadata via `HEAD` for an addressed resource (no body).
    pub async fn stats<A: IntoPubkyResource>(&self, addr: A) -> Result<Option<ResourceStats>> {
        let resp = self.request(Method::HEAD, addr).await?.send().await?;
        if resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::GONE {
            return Ok(None);
        }
        let resp = check_http_status(resp).await?;
        Ok(Some(ResourceStats::from_headers(resp.headers())))
    }
}
