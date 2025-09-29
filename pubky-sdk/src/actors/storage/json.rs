use reqwest::Response;

use super::core::{PublicStorage, SessionStorage};
use super::resource::{IntoPubkyResource, IntoResourcePath};
use crate::Result;
use crate::util::check_http_status;

//
// SessionStorage (as-me)
//

impl SessionStorage {
    /// GET and deserialize JSON from an **absolute path**.
    ///
    /// Sets `Accept: application/json` and returns `T` via `resp.json()`.
    pub async fn get_json<P, T>(&self, path: P) -> Result<T>
    where
        P: IntoResourcePath,
        T: serde::de::DeserializeOwned,
    {
        let resp = self
            .request(reqwest::Method::GET, path)
            .await?
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let resp = check_http_status(resp).await?;
        Ok(resp.json::<T>().await?)
    }

    /// PUT JSON to an **absolute path** and return the raw `Response`.
    ///
    /// Serializes `body` as JSON.
    pub async fn put_json<P, B>(&self, path: P, body: &B) -> Result<Response>
    where
        P: IntoResourcePath,
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

//
// PublicStorage (read-only)
//

impl PublicStorage {
    /// GET and deserialize JSON from an **addressed resource**.
    pub async fn get_json<A, T>(&self, addr: A) -> Result<T>
    where
        A: IntoPubkyResource,
        T: serde::de::DeserializeOwned,
    {
        let resp = self
            .request(reqwest::Method::GET, addr)
            .await?
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let resp = check_http_status(resp).await?;
        Ok(resp.json::<T>().await?)
    }
}
