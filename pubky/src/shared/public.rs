use bytes::Bytes;

use pkarr::PublicKey;
use reqwest::{Method, Response, StatusCode};
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
        let mut original_url: Url = url.try_into().map_err(|_| Error::InvalidUrl)?;

        if original_url.scheme() != "pubky" {
            return Ok(original_url);
        }

        let pubky = original_url
            .host_str()
            .ok_or(Error::Generic("Missing Pubky Url host".to_string()))?
            .to_string();

        let Endpoint { mut url, .. } = self
            .resolve_pubky_homeserver(&PublicKey::try_from(pubky.clone())?)
            .await?;

        let path = original_url.path_segments();

        // TODO: replace if we move to subdomains instead of paths.
        let mut split = url.path_segments_mut().unwrap();
        split.push(&pubky);
        if let Some(segments) = path {
            for segment in segments {
                split.push(segment);
            }
        }
        drop(split);

        Ok(url)
    }
}

fn normalize_path(path: &str) -> Result<String> {
    let mut path = path.to_string();

    if path.starts_with('/') {
        path = path[1..].to_string()
    }

    // TODO: should we return error instead?
    if path.ends_with('/') {
        path = path[..path.len()].to_string()
    }

    Ok(path)
}

#[cfg(test)]
mod tests {

    use core::panic;

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_homeserver::Homeserver;
    use reqwest::StatusCode;

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
                error => {
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
                error => {
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

        let urls = vec![
            format!("pubky://{}/pub/a.wrong/a.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/a.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
            format!(
                "pubky://{}/pub/example.com/cc-nested/z.txt",
                keypair.public_key()
            ),
            format!("pubky://{}/pub/example.wrong/a.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/d.txt", keypair.public_key()),
            format!("pubky://{}/pub/z.wrong/a.txt", keypair.public_key()),
        ];

        for url in urls {
            client.put(url.as_str(), &[0]).await.unwrap();
        }

        let url = format!("pubky://{}/pub/example.com/extra", keypair.public_key());
        let url = url.as_str();

        {
            let list = client.list(url).unwrap().send().await.unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{}/pub/example.com/a.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
                    format!(
                        "pubky://{}/pub/example.com/cc-nested/z.txt",
                        keypair.public_key()
                    ),
                    format!("pubky://{}/pub/example.com/d.txt", keypair.public_key()),
                ],
                "normal list with no limit or cursor"
            );
        }

        {
            let list = client.list(url).unwrap().limit(2).send().await.unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{}/pub/example.com/a.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
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
                    format!(
                        "pubky://{}/pub/example.com/cc-nested/z.txt",
                        keypair.public_key()
                    ),
                    format!("pubky://{}/pub/example.com/d.txt", keypair.public_key()),
                ],
                "normal list with limit and a directory cursor"
            );
        }

        {
            let list = client
                .list(url)
                .unwrap()
                .limit(2)
                .cursor(&format!(
                    "pubky://{}/pub/example.com/a.txt",
                    keypair.public_key()
                ))
                .send()
                .await
                .unwrap();

            assert_eq!(
                list,
                vec![
                    format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.com/d.txt", keypair.public_key()),
                    format!(
                        "pubky://{}/pub/example.com/cc-nested/z.txt",
                        keypair.public_key()
                    ),
                    format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/a.txt", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.com/d.txt", keypair.public_key()),
                    format!(
                        "pubky://{}/pub/example.com/cc-nested/z.txt",
                        keypair.public_key()
                    ),
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
                    format!(
                        "pubky://{}/pub/example.com/cc-nested/z.txt",
                        keypair.public_key()
                    ),
                    format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
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

        let urls = vec![
            format!("pubky://{}/pub/a.com/a.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/a.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/b.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/c.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.com/d.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.con/d.txt", keypair.public_key()),
            format!("pubky://{}/pub/example.con", keypair.public_key()),
            format!("pubky://{}/pub/file", keypair.public_key()),
            format!("pubky://{}/pub/file2", keypair.public_key()),
            format!("pubky://{}/pub/z.com/a.txt", keypair.public_key()),
        ];

        for url in urls {
            client.put(url.as_str(), &[0]).await.unwrap();
        }

        let url = format!("pubky://{}/pub/", keypair.public_key());
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
                    format!("pubky://{}/pub/a.com/", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/", keypair.public_key()),
                    format!("pubky://{}/pub/example.con", keypair.public_key()),
                    format!("pubky://{}/pub/example.con/", keypair.public_key()),
                    format!("pubky://{}/pub/file", keypair.public_key()),
                    format!("pubky://{}/pub/file2", keypair.public_key()),
                    format!("pubky://{}/pub/z.com/", keypair.public_key()),
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
                    format!("pubky://{}/pub/a.com/", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.com/", keypair.public_key()),
                    format!("pubky://{}/pub/example.con", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.con", keypair.public_key()),
                    format!("pubky://{}/pub/example.con/", keypair.public_key()),
                    format!("pubky://{}/pub/file", keypair.public_key()),
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
                    format!("pubky://{}/pub/z.com/", keypair.public_key()),
                    format!("pubky://{}/pub/file2", keypair.public_key()),
                    format!("pubky://{}/pub/file", keypair.public_key()),
                    format!("pubky://{}/pub/example.con/", keypair.public_key()),
                    format!("pubky://{}/pub/example.con", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/", keypair.public_key()),
                    format!("pubky://{}/pub/a.com/", keypair.public_key()),
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
                    format!("pubky://{}/pub/z.com/", keypair.public_key()),
                    format!("pubky://{}/pub/file2", keypair.public_key()),
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
                    format!("pubky://{}/pub/file", keypair.public_key()),
                    format!("pubky://{}/pub/example.con/", keypair.public_key()),
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
                    format!("pubky://{}/pub/example.con", keypair.public_key()),
                    format!("pubky://{}/pub/example.com/", keypair.public_key()),
                ],
                "reverse list shallow with limit and a directory cursor"
            );
        }
    }
}
