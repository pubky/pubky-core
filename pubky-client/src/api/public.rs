use anyhow::Result;
use reqwest::Method;

use crate::{Client, http_client::HttpClient};

// The implementation block is now generic over the HttpClient.
impl<H: HttpClient> Client<H> {
    /// Returns a `ListBuilder` to fluently construct a list request.
    ///
    /// # Arguments
    /// * `url` - The `pubky://` URL of the directory you want to list.
    pub fn list(&self, url: &str) -> ListBuilder<H> {
        ListBuilder::new(self, url)
    }
}

/// A fluent builder for creating a Pubky "list" API request.
#[derive(Debug)]
pub struct ListBuilder<'a, H: HttpClient> {
    client: &'a Client<H>,
    url: String,
    reverse: bool,
    limit: Option<u16>,
    cursor: Option<String>,
    shallow: bool,
}

// The ListBuilder implementation is now also generic.
impl<'a, H: HttpClient> ListBuilder<'a, H> {
    /// Creates a new `ListBuilder`.
    fn new(client: &'a Client<H>, url: &str) -> Self {
        Self {
            client,
            url: url.to_string(),
            limit: None,
            cursor: None,
            reverse: false,
            shallow: false,
        }
    }

    /// Set the `reverse` option to list items in reverse order.
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Set the `limit` for the number of items returned.
    pub fn limit(mut self, limit: u16) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set the `cursor` to paginate through results.
    /// This can be a full `pubky://` URL from a previous response or a relative path.
    pub fn cursor(mut self, cursor: &str) -> Self {
        self.cursor = Some(cursor.to_string());
        self
    }

    /// Set the `shallow` option to list only directories and files at the current
    /// level, rather than a flat list of all files recursively.
    pub fn shallow(mut self, shallow: bool) -> Self {
        self.shallow = shallow;
        self
    }

    /// Sends the configured list request.
    ///
    /// # Returns
    /// A `Vec<String>` where each string is a `pubky://` URL of an item in the directory.
    pub async fn send(self) -> Result<Vec<String>> {
        // Build the final URL with all query parameters.
        let mut url = url::Url::parse(&self.url)?;

        // The public API works on directories, which should end with a '/'.
        // This ensures we are always querying a directory path.
        if !url.path().ends_with('/') {
            let path = url.path().to_string();
            if let Some(parent) = std::path::Path::new(&path).parent() {
                if let Some(parent_str) = parent.to_str() {
                    let new_path = if parent_str.is_empty() {
                        "/".to_string()
                    } else {
                        format!("{}/", parent_str)
                    };
                    url.set_path(&new_path);
                }
            }
        }

        let mut query = url.query_pairs_mut();

        if self.reverse {
            query.append_key_only("reverse");
        }
        if self.shallow {
            query.append_key_only("shallow");
        }
        if let Some(limit) = self.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(cursor) = &self.cursor {
            query.append_pair("cursor", cursor);
        }
        drop(query);

        // Perform the request using the abstract client.request() method.
        let bytes = self.client.request(Method::GET, url.as_str(), None).await?;

        // Parse the response body into a list of URL strings.
        Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .map(String::from)
            .collect())
    }
}
