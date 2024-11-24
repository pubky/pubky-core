use url::Url;

use crate::{error::Result, shared::list_builder::ListBuilder, Client};

impl Client {
    /// Returns a [ListBuilder] to help pass options before calling [ListBuilder::send].
    ///
    /// `url` sets the path you want to lest within.
    pub fn list<T: TryInto<Url>>(&self, url: T) -> Result<ListBuilder> {
        self.inner_list(url)
    }
}
