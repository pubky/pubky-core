use reqwest::IntoUrl;

use anyhow::Result;

use crate::{shared::list_builder::ListBuilder, Client};

impl Client {
    /// Returns a [ListBuilder] to help pass options before calling [ListBuilder::send].
    ///
    /// `url` sets the path you want to lest within.
    pub fn list<T: IntoUrl>(&self, url: T) -> Result<ListBuilder> {
        self.inner_list(url)
    }
}
