use pkarr::PublicKey;
use url::Url;

use crate::{
    error::{Error, Result},
    Client,
};

use super::{list_builder::ListBuilder, pkarr::Endpoint};

impl Client {
    pub(crate) fn inner_list<T: TryInto<Url>>(&self, url: T) -> Result<ListBuilder> {
        Ok(ListBuilder::new(
            self,
            url.try_into().map_err(|_| Error::InvalidUrl)?,
        ))
    }

    pub(crate) async fn pubky_to_http<T: TryInto<Url>>(&self, url: T) -> Result<Url> {
        let original_url: Url = url.try_into().map_err(|_| Error::InvalidUrl)?;

        let pubky = original_url
            .host_str()
            .ok_or(Error::Generic("Missing Pubky Url host".to_string()))?;

        if let Ok(public_key) = PublicKey::try_from(pubky) {
            let Endpoint { mut url, .. } = self.resolve_pubky_homeserver(&public_key).await?;

            // TODO: remove if we move to subdomains instead of paths.
            if original_url.scheme() == "pubky" {
                let path = original_url.path_segments();

                let mut split = url.path_segments_mut().unwrap();
                split.push(pubky);
                if let Some(segments) = path {
                    for segment in segments {
                        split.push(segment);
                    }
                }
                drop(split);
            }

            return Ok(url);
        }

        Ok(original_url)
    }
}

#[cfg(test)]
mod tests {

    use crate::*;

    use bytes::Bytes;
    use pkarr::{mainline::Testnet, Keypair};
    use pubky_homeserver::Homeserver;
    use reqwest::{Method, StatusCode};

    #[tokio::test]
    async fn put_get_delete() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

        let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
        let url = url.as_str();

        client
            .put(url)
            .body(vec![0, 1, 2, 3, 4])
            .send()
            .await?
            .error_for_status()?;

        let response = client.get(url).send().await?.bytes().await?;

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

        client.delete(url).send().await?.error_for_status()?;

        let response = client.get(url).send().await?;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        Ok(())
    }

    #[tokio::test]
    async fn unauthorized_put_delete() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

        let public_key = keypair.public_key();

        let url = format!("pubky://{public_key}/pub/foo.txt");
        let url = url.as_str();

        let other_client = Client::test(&testnet);
        {
            let other = Keypair::random();

            // TODO: remove extra client after switching to subdomains.
            other_client.signup(&other, &server.public_key()).await?;

            assert_eq!(
                other_client
                    .put(url)
                    .body(vec![0, 1, 2, 3, 4])
                    .send()
                    .await?
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }

        client.put(url).body(vec![0, 1, 2, 3, 4]).send().await?;

        {
            let other = Keypair::random();

            // TODO: remove extra client after switching to subdomains.
            other_client.signup(&other, &server.public_key()).await?;

            assert_eq!(
                other_client.delete(url).send().await?.status(),
                StatusCode::UNAUTHORIZED
            );
        }

        let response = client.get(url).send().await?.bytes().await?;

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

        Ok(())
    }

    #[tokio::test]
    async fn list() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

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
            client.put(url).body(vec![0]).send().await?;
        }

        let url = format!("pubky://{pubky}/pub/example.com/extra");
        let url = url.as_str();

        {
            let list = client.list(url)?.send().await?;

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
            let list = client.list(url)?.limit(2).send().await?;

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
            let list = client.list(url)?.limit(2).cursor("a.txt").send().await?;

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
                .list(url)?
                .limit(2)
                .cursor("cc-nested/")
                .send()
                .await?;

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
                .list(url)?
                .limit(2)
                .cursor(&format!("pubky://{pubky}/pub/example.com/a.txt"))
                .send()
                .await?;

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
            let list = client.list(url)?.limit(2).cursor("/a.txt").send().await?;

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
            let list = client.list(url)?.reverse(true).send().await?;

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
            let list = client.list(url)?.reverse(true).limit(2).send().await?;

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
                .list(url)?
                .reverse(true)
                .limit(2)
                .cursor("d.txt")
                .send()
                .await?;

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                    format!("pubky://{pubky}/pub/example.com/c.txt"),
                ],
                "reverse list with limit and cursor"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn list_shallow() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

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
            client.put(url).body(vec![0]).send().await?;
        }

        let url = format!("pubky://{pubky}/pub/");
        let url = url.as_str();

        {
            let list = client.list(url)?.shallow(true).send().await?;

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
            let list = client.list(url)?.shallow(true).limit(2).send().await?;

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
                .list(url)?
                .shallow(true)
                .limit(2)
                .cursor("example.com/a.txt")
                .send()
                .await?;

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
                .list(url)?
                .shallow(true)
                .limit(3)
                .cursor("example.com/")
                .send()
                .await?;

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
            let list = client.list(url)?.reverse(true).shallow(true).send().await?;

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
                .list(url)?
                .reverse(true)
                .shallow(true)
                .limit(2)
                .send()
                .await?;

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
                .list(url)?
                .shallow(true)
                .reverse(true)
                .limit(2)
                .cursor("file2")
                .send()
                .await?;

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
                .list(url)?
                .shallow(true)
                .reverse(true)
                .limit(2)
                .cursor("example.con/")
                .send()
                .await?;

            assert_eq!(
                list,
                vec![
                    format!("pubky://{pubky}/pub/example.con"),
                    format!("pubky://{pubky}/pub/example.com/"),
                ],
                "reverse list shallow with limit and a directory cursor"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn list_events() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

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
            client.put(&url).body(vec![0]).send().await?;
            client.delete(url).send().await?;
        }

        let feed_url = format!("http://localhost:{}/events/", server.port());
        let feed_url = feed_url.as_str();

        let client = Client::test(&testnet);

        let cursor;

        {
            let response = client
                .request(Method::GET, format!("{feed_url}?limit=10"))
                .send()
                .await?;

            let text = response.text().await?;
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
                .await?;

            let text = response.text().await?;
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

        Ok(())
    }

    #[tokio::test]
    async fn read_after_event() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

        let pubky = keypair.public_key();

        let url = format!("pubky://{pubky}/pub/a.com/a.txt");
        let url = url.as_str();

        client.put(url).body(vec![0]).send().await?;

        let feed_url = format!("http://localhost:{}/events/", server.port());
        let feed_url = feed_url.as_str();

        let client = Client::test(&testnet);

        {
            let response = client
                .request(Method::GET, format!("{feed_url}?limit=10"))
                .send()
                .await?;

            let text = response.text().await?;
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

        let get = client.get(url).send().await?.bytes().await?;

        assert_eq!(get.as_ref(), &[0]);

        Ok(())
    }

    #[tokio::test]
    async fn dont_delete_shared_blobs() -> anyhow::Result<()> {
        let testnet = Testnet::new(10)?;
        let homeserver = Homeserver::start_test(&testnet).await?;
        let client = Client::test(&testnet);

        let homeserver_pubky = homeserver.public_key();

        let user_1 = Keypair::random();
        let user_2 = Keypair::random();

        client.signup(&user_1, &homeserver_pubky).await?;
        client.signup(&user_2, &homeserver_pubky).await?;

        let user_1_id = user_1.public_key();
        let user_2_id = user_2.public_key();

        let url_1 = format!("pubky://{user_1_id}/pub/pubky.app/file/file_1");
        let url_2 = format!("pubky://{user_2_id}/pub/pubky.app/file/file_1");

        let file = vec![1];
        client.put(&url_1).body(file.clone()).send().await?;
        client.put(&url_2).body(file.clone()).send().await?;

        // Delete file 1
        client.delete(url_1).send().await?.error_for_status()?;

        let blob = client.get(url_2).send().await?.bytes().await?;

        assert_eq!(blob, file);

        let feed_url = format!("http://localhost:{}/events/", homeserver.port());

        let response = client
            .request(Method::GET, format!("{feed_url}"))
            .send()
            .await?;

        let text = response.text().await?;
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

        Ok(())
    }

    #[tokio::test]
    async fn stream() -> anyhow::Result<()> {
        // TODO: test better streaming API

        let testnet = Testnet::new(10)?;
        let server = Homeserver::start_test(&testnet).await?;

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await?;

        let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
        let url = url.as_str();

        let bytes = Bytes::from(vec![0; 1024 * 1024]);

        client.put(url).body(bytes.clone()).send().await?;

        let response = client.get(url).send().await?.bytes().await?;

        assert_eq!(response, bytes);

        client.delete(url).send().await?;

        let response = client.get(url).send().await?;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        Ok(())
    }
}
