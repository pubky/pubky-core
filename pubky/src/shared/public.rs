use bytes::Bytes;

use pkarr::PublicKey;
use reqwest::{Method, StatusCode};
use url::Url;

use crate::{
    error::{Error, Result},
    PubkyClient,
};

use super::{list_builder::ListBuilder, pkarr::Endpoint};

impl PubkyClient {
    pub(crate) async fn inner_put<T: TryInto<Url>>(&self, url: T, content: &[u8]) -> Result<()> {
        let url = self.pubky_to_http(url).await?;

        let response = self
            .request(Method::PUT, url)
            .body(content.to_owned())
            .send()
            .await?;

        response.error_for_status()?;

        Ok(())
    }

    pub(crate) async fn inner_get<T: TryInto<Url>>(&self, url: T) -> Result<Option<Bytes>> {
        let url = self.pubky_to_http(url).await?;

        let response = self.request(Method::GET, url).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        response.error_for_status_ref()?;

        // TODO: bail on too large files.
        let bytes = response.bytes().await?;

        Ok(Some(bytes))
    }

    pub(crate) async fn inner_delete<T: TryInto<Url>>(&self, url: T) -> Result<()> {
        let url = self.pubky_to_http(url).await?;

        let response = self.request(Method::DELETE, url).send().await?;

        response.error_for_status_ref()?;

        Ok(())
    }

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

    use core::panic;

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_homeserver::Homeserver;
    use reqwest::{Method, StatusCode};

    #[tokio::test]
    async fn put_get_delete() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
        let url = url.as_str();

        client.put(url, &[0, 1, 2, 3, 4]).await.unwrap();

        let response = client.get(url).await.unwrap().unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

        client.delete(url).await.unwrap();

        let response = client.get(url).await.unwrap();

        assert_eq!(response, None);
    }

    #[tokio::test]
    async fn unauthorized_put_delete() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let public_key = keypair.public_key();

        let url = format!("pubky://{public_key}/pub/foo.txt");
        let url = url.as_str();

        let other_client = PubkyClient::test(&testnet);
        {
            let other = Keypair::random();

            // TODO: remove extra client after switching to subdomains.
            other_client
                .signup(&other, &server.public_key())
                .await
                .unwrap();

            let response = other_client.put(url, &[0, 1, 2, 3, 4]).await;

            match response {
                Err(Error::Reqwest(error)) => {
                    assert!(error.status() == Some(StatusCode::UNAUTHORIZED))
                }
                _ => {
                    panic!("expected error StatusCode::UNAUTHORIZED")
                }
            }
        }

        client.put(url, &[0, 1, 2, 3, 4]).await.unwrap();

        {
            let other = Keypair::random();

            // TODO: remove extra client after switching to subdomains.
            other_client
                .signup(&other, &server.public_key())
                .await
                .unwrap();

            let response = other_client.delete(url).await;

            match response {
                Err(Error::Reqwest(error)) => {
                    assert!(error.status() == Some(StatusCode::UNAUTHORIZED))
                }
                _ => {
                    panic!("expected error StatusCode::UNAUTHORIZED")
                }
            }
        }

        let response = client.get(url).await.unwrap().unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));
    }

    #[tokio::test]
    async fn list() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

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
            client.put(url.as_str(), &[0]).await.unwrap();
        }

        let url = format!("pubky://{pubky}/pub/example.com/extra");
        let url = url.as_str();

        {
            let list = client.list(url).unwrap().send().await.unwrap();

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
            let list = client.list(url).unwrap().limit(2).send().await.unwrap();

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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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

    #[tokio::test]
    async fn list_shallow() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

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
            client.put(url.as_str(), &[0]).await.unwrap();
        }

        let url = format!("pubky://{pubky}/pub/");
        let url = url.as_str();

        {
            let list = client
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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
                .list(url)
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

    #[tokio::test]
    async fn list_events() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

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
            client.put(url.as_str(), &[0]).await.unwrap();
            client.delete(url.as_str()).await.unwrap();
        }

        let feed_url = format!("http://localhost:{}/events/", server.port());
        let feed_url = feed_url.as_str();

        let client = PubkyClient::test(&testnet);

        let cursor;

        {
            let response = client
                .request(
                    Method::GET,
                    format!("{feed_url}?limit=10").as_str().try_into().unwrap(),
                )
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
                .request(
                    Method::GET,
                    format!("{feed_url}?limit=10&cursor={cursor}")
                        .as_str()
                        .try_into()
                        .unwrap(),
                )
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

    #[tokio::test]
    async fn read_after_event() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let pubky = keypair.public_key();

        let url = format!("pubky://{pubky}/pub/a.com/a.txt");

        client.put(url.as_str(), &[0]).await.unwrap();

        let feed_url = format!("http://localhost:{}/events/", server.port());
        let feed_url = feed_url.as_str();

        let client = PubkyClient::test(&testnet);

        {
            let response = client
                .request(
                    Method::GET,
                    format!("{feed_url}?limit=10").as_str().try_into().unwrap(),
                )
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

        let get = client.get(url.as_str()).await.unwrap();
        dbg!(get);
    }

    #[tokio::test]
    async fn dont_delete_shared_blobs() {
        let testnet = Testnet::new(10);
        let homeserver = Homeserver::start_test(&testnet).await.unwrap();
        let client = PubkyClient::test(&testnet);

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
        client.put(url_1.as_str(), &file).await.unwrap();
        client.put(url_2.as_str(), &file).await.unwrap();

        // Delete file 1
        client.delete(url_1.as_str()).await.unwrap();

        let blob = client.get(url_2.as_str()).await.unwrap().unwrap();

        assert_eq!(blob, file);

        let feed_url = format!("http://localhost:{}/events/", homeserver.port());

        let response = client
            .request(
                Method::GET,
                format!("{feed_url}").as_str().try_into().unwrap(),
            )
            .send()
            .await
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
        )
    }
}
