use reqwest::{IntoUrl, Method};

use anyhow::Result;

use crate::handle_http_error;

use super::super::Client;

impl Client {
    /// Returns a [ListBuilder] to help pass options before calling [ListBuilder::send].
    ///
    /// `url` sets the path you want to lest within.
    pub fn list<T: IntoUrl>(&self, url: T) -> Result<ListBuilder> {
        Ok(ListBuilder::new(self, url))
    }
}

/// Helper struct to edit Pubky homeserver's list API options before sending them.
#[derive(Debug)]
pub struct ListBuilder<'a> {
    url: String,
    reverse: bool,
    limit: Option<u16>,
    cursor: Option<&'a str>,
    client: &'a Client,
    shallow: bool,
}

impl<'a> ListBuilder<'a> {
    /// Create a new List request builder
    pub(crate) fn new<T: IntoUrl>(client: &'a Client, url: T) -> Self {
        Self {
            client,
            url: url.as_str().to_string(),
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
        let mut url = url::Url::parse(&self.url)?;

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

        let response = self
            .client
            .cross_request(Method::GET, url)
            .await
            .send()
            .await?;

        handle_http_error!(response);

        // TODO: bail on too large files.
        let bytes = response.bytes().await?;

        Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .map(String::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use pkarr::Keypair;
    use pubky_testnet::Testnet;
    use reqwest::{Method, StatusCode};

    #[tokio_shared_rt::test(shared)]
    async fn put_get_delete() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
        let url = url.as_str();

        client
            .put(url)
            .body(vec![0, 1, 2, 3, 4])
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let response = client.get(url).send().await.unwrap().bytes().await.unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

        client
            .delete(url)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let response = client.get(url).send().await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio_shared_rt::test(shared)]
    async fn unauthorized_put_delete() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let public_key = keypair.public_key();

        let url = format!("pubky://{public_key}/pub/foo.txt");
        let url = url.as_str();

        let other_client = testnet.client_builder().build().unwrap();
        {
            let other = Keypair::random();

            // TODO: remove extra client after switching to subdomains.
            other_client
                .signup(&other, &server.public_key())
                .await
                .unwrap();

            assert_eq!(
                other_client
                    .put(url)
                    .body(vec![0, 1, 2, 3, 4])
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }

        client
            .put(url)
            .body(vec![0, 1, 2, 3, 4])
            .send()
            .await
            .unwrap();

        {
            let other = Keypair::random();

            // TODO: remove extra client after switching to subdomains.
            other_client
                .signup(&other, &server.public_key())
                .await
                .unwrap();

            assert_eq!(
                other_client.delete(url).send().await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
        }

        let response = client.get(url).send().await.unwrap().bytes().await.unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));
    }

    #[tokio_shared_rt::test(shared)]
    async fn list() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let pubky = keypair.public_key();

        let urls = vec![
            format!("pubky://{pubky}/pub/a.wrong/a.txt"),
            format!("pubky://{pubky}/pub/example.com/a.txt"),
            format!("pubky://{pubky}/pub/example.com/b.txt"),
            format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
            format!("pubky://{pubky}/pub/example.wrong/a.txt"),
            format!("pubky://{pubky}/pub/example.com/c.txt"),
            format!("pubky://{pubky}/pub/example.com/d.txt"),
            format!("pubky://{pubky}/pub/z.wrong/a.txt"),
        ];

        for url in urls {
            client.put(url).body(vec![0]).send().await.unwrap();
        }

        let url = format!("pubky://{pubky}/pub/example.com/extra");

        {
            let list = client.list(&url).unwrap().send().await.unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/a.txt"),
                    format!("pubky://{pubky}/pub/example.com/b.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                    format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                    format!("pubky://{pubky}/pub/example.com/d.txt"),
                ],
                "normal list with no limit or cursor"
            );
        }

        {
            let list = client.list(&url).unwrap().limit(2).send().await.unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/a.txt"),
                    format!("pubky://{pubky}/pub/example.com/b.txt"),
                ],
                "normal list with limit but no cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .limit(2)
                .cursor("a.txt")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/b.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                ],
                "normal list with limit and a file cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .limit(2)
                .cursor("cc-nested/")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                    format!("pubky://{pubky}/pub/example.com/d.txt"),
                ],
                "normal list with limit and a directory cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .limit(2)
                .cursor(&format!("pubky://{pubky}/pub/example.com/a.txt"))
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/b.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                ],
                "normal list with limit and a full url cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .limit(2)
                .cursor("/a.txt")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/b.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                ],
                "normal list with limit and a leading / cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .reverse(true)
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/d.txt"),
                    format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                    format!("pubky://{pubky}/pub/example.com/b.txt"),
                    format!("pubky://{pubky}/pub/example.com/a.txt"),
                ],
                "reverse list with no limit or cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .reverse(true)
                .limit(2)
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/d.txt"),
                    format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                ],
                "reverse list with limit but no cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .reverse(true)
                .limit(2)
                .cursor("d.txt")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                ],
                "reverse list with limit and cursor"
            );
        }
    }

    #[tokio_shared_rt::test(shared)]
    async fn list_shallow() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let pubky = keypair.public_key();

        let urls = vec![
            format!("pubky://{pubky}/pub/a.com/a.txt"),
            format!("pubky://{pubky}/pub/example.com/a.txt"),
            format!("pubky://{pubky}/pub/example.com/b.txt"),
            format!("pubky://{pubky}/pub/example.com/c.txt"),
            format!("pubky://{pubky}/pub/example.com/d.txt"),
            format!("pubky://{pubky}/pub/example.con/d.txt"),
            format!("pubky://{pubky}/pub/example.con"),
            format!("pubky://{pubky}/pub/file"),
            format!("pubky://{pubky}/pub/file2"),
            format!("pubky://{pubky}/pub/z.com/a.txt"),
        ];

        for url in urls {
            client.put(url).body(vec![0]).send().await.unwrap();
        }

        let url = format!("pubky://{pubky}/pub/");

        {
            let list = client
                .list(&url)
                .unwrap()
                .shallow(true)
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/a.com/"),
                    format!("pubky://{pubky}/pub/example.com/"),
                    format!("pubky://{pubky}/pub/example.con"),
                    format!("pubky://{pubky}/pub/example.con/"),
                    format!("pubky://{pubky}/pub/file"),
                    format!("pubky://{pubky}/pub/file2"),
                    format!("pubky://{pubky}/pub/z.com/"),
                ],
                "normal list shallow"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .shallow(true)
                .limit(2)
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/a.com/"),
                    format!("pubky://{pubky}/pub/example.com/"),
                ],
                "normal list shallow with limit but no cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .shallow(true)
                .limit(2)
                .cursor("example.com/a.txt")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/"),
                    format!("pubky://{pubky}/pub/example.con"),
                ],
                "normal list shallow with limit and a file cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .shallow(true)
                .limit(3)
                .cursor("example.com/")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.con"),
                    format!("pubky://{pubky}/pub/example.con/"),
                    format!("pubky://{pubky}/pub/file"),
                ],
                "normal list shallow with limit and a directory cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .reverse(true)
                .shallow(true)
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/z.com/"),
                    format!("pubky://{pubky}/pub/file2"),
                    format!("pubky://{pubky}/pub/file"),
                    format!("pubky://{pubky}/pub/example.con/"),
                    format!("pubky://{pubky}/pub/example.con"),
                    format!("pubky://{pubky}/pub/example.com/"),
                    format!("pubky://{pubky}/pub/a.com/"),
                ],
                "reverse list shallow"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .reverse(true)
                .shallow(true)
                .limit(2)
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/z.com/"),
                    format!("pubky://{pubky}/pub/file2"),
                ],
                "reverse list shallow with limit but no cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .shallow(true)
                .reverse(true)
                .limit(2)
                .cursor("file2")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/file"),
                    format!("pubky://{pubky}/pub/example.con/"),
                ],
                "reverse list shallow with limit and a file cursor"
            );
        }

        {
            let list = client
                .list(&url)
                .unwrap()
                .shallow(true)
                .reverse(true)
                .limit(2)
                .cursor("example.con/")
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.con"),
                    format!("pubky://{pubky}/pub/example.com/"),
                ],
                "reverse list shallow with limit and a directory cursor"
            );
        }
    }

    #[tokio_shared_rt::test(shared)]
    async fn list_events() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let pubky = keypair.public_key();

        let urls = vec![
            format!("pubky://{pubky}/pub/a.com/a.txt"),
            format!("pubky://{pubky}/pub/example.com/a.txt"),
            format!("pubky://{pubky}/pub/example.com/b.txt"),
            format!("pubky://{pubky}/pub/example.com/c.txt"),
            format!("pubky://{pubky}/pub/example.com/d.txt"),
            format!("pubky://{pubky}/pub/example.con/d.txt"),
            format!("pubky://{pubky}/pub/example.con"),
            format!("pubky://{pubky}/pub/file"),
            format!("pubky://{pubky}/pub/file2"),
            format!("pubky://{pubky}/pub/z.com/a.txt"),
        ];

        for url in urls {
            client.put(&url).body(vec![0]).send().await.unwrap();
            client.delete(url).send().await.unwrap();
        }

        let feed_url = format!("https://{}/events/", server.public_key());

        let client = testnet.client_builder().build().unwrap();

        let cursor;

        {
            let response = client
                .request(Method::GET, format!("{feed_url}?limit=10"))
                .send()
                .await
                .unwrap();

            let text = response.text().await.unwrap();
            let lines = text.split('\n').collect::<Vec<_>>();

            cursor = lines.last().unwrap().split(" ").last().unwrap().to_string();

            assert_eq!(
                lines,
                vec![
                    format!("PUT pubky://{pubky}/pub/a.com/a.txt"),
                    format!("DEL pubky://{pubky}/pub/a.com/a.txt"),
                    format!("PUT pubky://{pubky}/pub/example.com/a.txt"),
                    format!("DEL pubky://{pubky}/pub/example.com/a.txt"),
                    format!("PUT pubky://{pubky}/pub/example.com/b.txt"),
                    format!("DEL pubky://{pubky}/pub/example.com/b.txt"),
                    format!("PUT pubky://{pubky}/pub/example.com/c.txt"),
                    format!("DEL pubky://{pubky}/pub/example.com/c.txt"),
                    format!("PUT pubky://{pubky}/pub/example.com/d.txt"),
                    format!("DEL pubky://{pubky}/pub/example.com/d.txt"),
                    format!("cursor: {cursor}",)
                ]
            );
        }

        {
            let response = client
                .request(Method::GET, format!("{feed_url}?limit=10&cursor={cursor}"))
                .send()
                .await
                .unwrap();

            let text = response.text().await.unwrap();
            let lines = text.split('\n').collect::<Vec<_>>();

            assert_eq!(
                lines,
                vec![
                    format!("PUT pubky://{pubky}/pub/example.con/d.txt"),
                    format!("DEL pubky://{pubky}/pub/example.con/d.txt"),
                    format!("PUT pubky://{pubky}/pub/example.con"),
                    format!("DEL pubky://{pubky}/pub/example.con"),
                    format!("PUT pubky://{pubky}/pub/file"),
                    format!("DEL pubky://{pubky}/pub/file"),
                    format!("PUT pubky://{pubky}/pub/file2"),
                    format!("DEL pubky://{pubky}/pub/file2"),
                    format!("PUT pubky://{pubky}/pub/z.com/a.txt"),
                    format!("DEL pubky://{pubky}/pub/z.com/a.txt"),
                    lines.last().unwrap().to_string()
                ]
            )
        }
    }

    #[tokio_shared_rt::test(shared)]
    async fn read_after_event() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let pubky = keypair.public_key();

        let url = format!("pubky://{pubky}/pub/a.com/a.txt");

        client.put(&url).body(vec![0]).send().await.unwrap();

        let feed_url = format!("https://{}/events/", server.public_key());

        let client = testnet.client_builder().build().unwrap();

        {
            let response = client
                .request(Method::GET, format!("{feed_url}?limit=10"))
                .send()
                .await
                .unwrap();

            let text = response.text().await.unwrap();
            let lines = text.split('\n').collect::<Vec<_>>();

            let cursor = lines.last().unwrap().split(" ").last().unwrap().to_string();

            assert_eq!(
                lines,
                vec![
                    format!("PUT pubky://{pubky}/pub/a.com/a.txt"),
                    format!("cursor: {cursor}",)
                ]
            );
        }

        let response = client.get(url).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.bytes().await.unwrap();

        assert_eq!(body.as_ref(), &[0]);
    }

    #[tokio_shared_rt::test(shared)]
    async fn dont_delete_shared_blobs() {
        let testnet = Testnet::run().await.unwrap();
        let homeserver = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let homeserver_pubky = homeserver.public_key();

        let user_1 = Keypair::random();
        let user_2 = Keypair::random();

        client.signup(&user_1, &homeserver_pubky).await.unwrap();
        client.signup(&user_2, &homeserver_pubky).await.unwrap();

        let user_1_id = user_1.public_key();
        let user_2_id = user_2.public_key();

        let url_1 = format!("pubky://{user_1_id}/pub/pubky.app/file/file_1");
        let url_2 = format!("pubky://{user_2_id}/pub/pubky.app/file/file_1");

        let file = vec![1];
        client.put(&url_1).body(file.clone()).send().await.unwrap();
        client.put(&url_2).body(file.clone()).send().await.unwrap();

        // Delete file 1
        client
            .delete(url_1)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let blob = client
            .get(url_2)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        assert_eq!(blob, file);

        let feed_url = format!("https://{}/events/", homeserver.public_key());

        let response = client
            .request(Method::GET, feed_url)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let text = response.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec![
                format!("PUT pubky://{user_1_id}/pub/pubky.app/file/file_1",),
                format!("PUT pubky://{user_2_id}/pub/pubky.app/file/file_1",),
                format!("DEL pubky://{user_1_id}/pub/pubky.app/file/file_1",),
                lines.last().unwrap().to_string()
            ]
        );
    }

    #[tokio_shared_rt::test(shared)]
    async fn stream() {
        // TODO: test better streaming API
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
        let url = url.as_str();

        let bytes = Bytes::from(vec![0; 1024 * 1024]);

        client.put(url).body(bytes.clone()).send().await.unwrap();

        let response = client.get(url).send().await.unwrap().bytes().await.unwrap();

        assert_eq!(response, bytes);

        client.delete(url).send().await.unwrap();

        let response = client.get(url).send().await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
