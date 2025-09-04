use reqwest::header::HeaderMap;
use reqwest::{Method, Response, StatusCode};

use super::core::PubkyDrive;
use super::path::IntoPubkyPath;

use crate::Result;
use crate::util::check_http_status;

impl PubkyDrive {
    fn err_if_require_session_for_write(&self) -> Result<()> {
        if self.has_session {
            return Ok(());
        }
        Err(crate::errors::AuthError::Validation(
            "write requires an authenticated session (use agent.drive())".into(),
        )
        .into())
    }

    /// HTTP `GET`.
    ///
    /// - In **session mode**, attaches the agent’s cookie when targeting that user’s homeserver.
    /// - In **public mode**, unauthenticated read against user-qualified paths.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// let resp = drive.get("/pub/app/data.bin").await?;
    /// let bytes = resp.bytes().await?;
    /// # Ok(()) }
    /// ```
    pub async fn get<P: IntoPubkyPath>(&self, path: P) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        check_http_status(resp).await
    }

    /// Lightweight existence check (no body download).
    ///
    /// Issues a `HEAD` request after resolving `path`. Returns:
    /// - `Ok(true)` for 2xx
    /// - `Ok(false)` for 404/410
    /// - `Err(..)` for any other status or transport error
    ///
    /// Works in public or session mode. In session mode, attaches the agent’s cookie
    /// when the URL targets this agent’s homeserver.
    pub async fn exists<P: IntoPubkyPath>(&self, path: P) -> Result<bool> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        match resp.status() {
            s if s.is_success() => Ok(true),
            StatusCode::NOT_FOUND | StatusCode::GONE => Ok(false),
            _ => {
                // Map non-success/404/410 via our helper (returns Err).
                let _ = check_http_status(resp).await?;
                // Unreachable: check_http_status above would have errored.
                Ok(false)
            }
        }
    }

    /// Retrieve metadata via `HEAD` (no body).
    ///
    /// Returns the response headers for existing resources, or `Ok(None)` for
    /// 404/410. Other non-2xx statuses (and transport errors) are returned as
    /// errors. Typical headers include `content-length`, `content-type`, `etag`,
    /// and `last-modified`.
    ///
    /// Works in public or session mode. In session mode, attaches the agent’s cookie
    /// when the URL targets this agent’s homeserver.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// if let Some(h) = drive.stats("/pub/app/data.bin").await? {
    ///     if let Some(len) = h.get(reqwest::header::CONTENT_LENGTH) {
    ///         println!("size: {}", len.to_str().unwrap_or("?"));
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    pub async fn stats<P: IntoPubkyPath>(&self, path: P) -> Result<Option<HeaderMap>> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        if resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::GONE {
            return Ok(None);
        }
        let resp = check_http_status(resp).await?;
        Ok(Some(resp.headers().clone()))
    }

    /// HTTP `PUT`.
    ///
    /// Requires a session (server authorization). In public mode, this request will be
    /// rejected, as servers will reject writes (401/403).
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// drive.put("/pub/app/hello.txt", "hello").await?;
    /// # Ok(()) }
    /// ```
    pub async fn put<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoPubkyPath,
        B: Into<reqwest::Body>,
    {
        self.err_if_require_session_for_write()?;
        let resp = self
            .request(Method::PUT, path)
            .await?
            .body(body)
            .send()
            .await?;
        check_http_status(resp).await
    }

    /// HTTP `POST`.
    ///
    /// Requires a session (server authorization). In public mode, this request will be
    /// rejected, as servers will reject writes (401/403).
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// drive.post("/pub/app/hello.txt", "hello").await?;
    /// # Ok(()) }
    /// ```
    pub async fn post<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoPubkyPath,
        B: Into<reqwest::Body>,
    {
        self.err_if_require_session_for_write()?;
        let resp = self
            .request(Method::POST, path)
            .await?
            .body(body)
            .send()
            .await?;
        check_http_status(resp).await
    }

    /// HTTP `PATCH`.
    ///
    /// Requires a session (server authorization). In public mode, this request will be
    /// rejected, as servers will reject writes (401/403).
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// drive.patch("/pub/app/hello.txt", "hello").await?;
    /// # Ok(()) }
    /// ```
    pub async fn patch<P, B>(&self, path: P, body: B) -> Result<Response>
    where
        P: IntoPubkyPath,
        B: Into<reqwest::Body>,
    {
        self.err_if_require_session_for_write()?;
        let resp = self
            .request(Method::PATCH, path)
            .await?
            .body(body)
            .send()
            .await?;
        check_http_status(resp).await
    }

    /// HTTP `DELETE`.
    ///
    /// Requires a session (server authorization). In public mode, this request will be
    /// rejected, as servers will reject writes (401/403).
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// drive.delete("/pub/app/hello.txt").await?;
    /// # Ok(()) }
    /// ```
    pub async fn delete<P: IntoPubkyPath>(&self, path: P) -> Result<Response> {
        self.err_if_require_session_for_write()?;
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        check_http_status(resp).await
    }
}
