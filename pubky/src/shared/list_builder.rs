use reqwest::{Method, Response, StatusCode};
use url::Url;

use crate::{error::Result, PubkyClient};

#[derive(Debug)]
pub struct ListBuilder<'a> {
    url: Url,
    reverse: bool,
    limit: Option<u16>,
    cursor: Option<&'a str>,
    client: &'a PubkyClient,
}

impl<'a> ListBuilder<'a> {
    pub fn new(client: &'a PubkyClient, url: Url) -> Self {
        Self {
            client,
            url,
            limit: None,
            cursor: None,
            reverse: false,
        }
    }

    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    pub fn limit(mut self, limit: u16) -> Self {
        self.limit = limit.into();
        self
    }

    pub fn cursor(mut self, cursor: &'a str) -> Self {
        self.cursor = cursor.into();
        self
    }

    pub async fn send(self) -> Result<Vec<String>> {
        let mut url = self.client.pubky_to_http(self.url).await?;

        let mut query = url.query_pairs_mut();
        query.append_key_only("list");

        if self.reverse {
            query.append_key_only("reverse");
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
