use reqwest::{Method, Response};

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

    /// HTTP `HEAD`.
    ///
    /// - In **session mode**, attaches the agent’s cookie when targeting that user’s homeserver.
    /// - In **public mode**, unauthenticated read against user-qualified paths.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyDrive) -> pubky::Result<()> {
    /// let resp = drive.head("/pub/app/data.bin").await?;
    /// let bytes = resp.bytes().await?;
    /// # Ok(()) }
    /// ```
    pub async fn head<P: IntoPubkyPath>(&self, path: P) -> Result<Response> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        check_http_status(resp).await
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
