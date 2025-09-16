use reqwest::Response;

use super::core::PubkyStorage;
use super::resource::IntoPubkyResource;

use crate::Result;
use crate::util::check_http_status;

impl PubkyStorage {
    /// GET and deserialize JSON.
    ///
    /// Sets `Accept: application/json` and returns `T` via `resp.json()`.
    ///
    /// # Examples
    /// ```no_run
    /// # use serde::Deserialize;
    /// # #[derive(Deserialize)] struct Info { version: String }
    /// # async fn ex(storage: pubky::PubkyStorage) -> pubky::Result<()> {
    /// let info: Info = storage.get_json("/pub/app/info.json").await?;
    /// # Ok(()) }
    /// ```
    pub async fn get_json<P, T>(&self, path: P) -> Result<T>
    where
        P: IntoPubkyResource,
        T: serde::de::DeserializeOwned,
    {
        let resp = self
            .request(reqwest::Method::GET, path)
            .await?
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let resp = crate::util::check_http_status(resp).await?;
        Ok(resp.json::<T>().await?)
    }

    /// PUT JSON and return the raw `Response`.
    ///
    /// Serializes `body` as JSON.
    /// Require an authenticated session for writes.
    ///
    /// # Examples
    /// ```no_run
    /// # use serde::Serialize;
    /// # #[derive(Serialize)] struct Info { version: String }
    /// # async fn ex(storage: pubky::PubkyStorage) -> pubky::Result<()> {
    /// let info = Info { version: "42".into() };
    /// storage.put_json("/pub/app/info.json", &info).await?;
    /// # Ok(()) }
    /// ```
    pub async fn put_json<P, B>(&self, path: P, body: &B) -> Result<Response>
    where
        P: IntoPubkyResource,
        B: serde::Serialize + ?Sized,
    {
        let resp = self
            .request(reqwest::Method::PUT, path)
            .await?
            .json(body)
            .send()
            .await?;
        check_http_status(resp).await
    }
}
