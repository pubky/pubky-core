use bytes::Bytes;
use url::Url;

use crate::{error::Result, shared::list_builder::ListBuilder, Client};

impl Client {
    /// Upload a small payload to a given path.
    pub async fn put<T: TryInto<Url>>(&self, url: T, content: &[u8]) -> Result<()> {
        self.inner_put(url, content).await
    }

    /// Download a small payload from a given path relative to a pubky author.
    pub async fn get<T: TryInto<Url>>(&self, url: T) -> Result<Option<Bytes>> {
        self.inner_get(url).await
    }

    /// Delete a file at a path relative to a pubky author.
    pub async fn delete<T: TryInto<Url>>(&self, url: T) -> Result<()> {
        self.inner_delete(url).await
    }

    /// Returns a [ListBuilder] to help pass options before calling [ListBuilder::send].
    ///
    /// `url` sets the path you want to lest within.
    pub fn list<T: TryInto<Url>>(&self, url: T) -> Result<ListBuilder> {
        self.inner_list(url)
    }
}
