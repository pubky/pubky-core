use reqwest::Method;
use url::Url;

use crate::{error::Result, PubkyClient};

#[derive(Debug)]
pub struct ListBuilder<'a> {
    url: Url,
    reverse: bool,
    limit: Option<u16>,
    cursor: Option<&'a str>,
    client: &'a PubkyClient,
    shallow: bool,
}

impl<'a> ListBuilder<'a> {
    /// Create a new List request builder
    pub(crate) fn new(client: &'a PubkyClient, url: Url) -> Self {
        Self {
            client,
            url,
            limit: None,
            cursor: None,
            reverse: false,
            shallow: false,
        }
    }

    /// Set the `reverse` option.
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Set the `limit` value.
    pub fn limit(mut self, limit: u16) -> Self {
        self.limit = limit.into();
        self
    }

    /// Set the `cursor` value.
    ///
    /// Either a full `pubky://` Url (from previous list response),
    /// or a path (to a file or directory) relative to the `url`
    pub fn cursor(mut self, cursor: &'a str) -> Self {
        self.cursor = cursor.into();
        self
    }

    pub fn shallow(mut self, shallow: bool) -> Self {
        self.shallow = shallow;
        self
    }

    /// Send the list request.
    ///
    /// Returns a list of Pubky URLs of the files in the path of the `url`
    /// respecting [ListBuilder::reverse], [ListBuilder::limit] and [ListBuilder::cursor]
    /// options.
    pub async fn send(self) -> Result<Vec<String>> {
        let mut url = self.client.pubky_to_http(self.url).await?;

        if !url.path().ends_with('/') {
            let path = url.path().to_string();
            let mut parts = path.split('/').collect::<Vec<&str>>();
            parts.pop();

            let path = format!("{}/", parts.join("/"));

            url.set_path(&path)
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

        if let Some(cursor) = self.cursor {
            query.append_pair("cursor", cursor);
        }

        drop(query);

        let response = self.client.request(Method::GET, url).send().await?;

        response.error_for_status_ref()?;

        // TODO: bail on too large files.
        let bytes = response.bytes().await?;

        Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .map(String::from)
            .collect())
    }
}
