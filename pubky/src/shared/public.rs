use bytes::Bytes;

use pkarr::PublicKey;
use reqwest::{Method, Response, StatusCode};
use url::Url;

use crate::{error::Result, PubkyClient};

impl PubkyClient {
    pub async fn inner_put(&self, pubky: &PublicKey, path: &str, content: &[u8]) -> Result<()> {
        let url = self.url(pubky, path).await?;

        let response = self
            .request(Method::PUT, url)
            .body(content.to_owned())
            .send()
            .await?;

        response.error_for_status()?;

        Ok(())
    }

    pub async fn inner_get(&self, pubky: &PublicKey, path: &str) -> Result<Option<Bytes>> {
        let url = self.url(pubky, path).await?;

        let response = self.request(Method::GET, url).send().await?;

        response.error_for_status_ref()?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        // TODO: bail on too large files.
        let bytes = response.bytes().await?;

        Ok(Some(bytes))
    }

    pub async fn inner_delete(&self, pubky: &PublicKey, path: &str) -> Result<()> {
        let url = self.url(pubky, path).await?;

        let response = self.request(Method::DELETE, url).send().await?;

        response.error_for_status_ref()?;

        Ok(())
    }

    async fn url(&self, pubky: &PublicKey, path: &str) -> Result<Url> {
        let path = normalize_path(path)?;

        let (_, mut url) = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{pubky}/{path}"));

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

        client
            .put(&keypair.public_key(), "/pub/foo.txt", &[0, 1, 2, 3, 4])
            .await
            .unwrap();

        let response = client
            .get(&keypair.public_key(), "/pub/foo.txt")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

        // client
        // .delete(&keypair.public_key(), "/pub/foo.txt")
        //     .await
        //     .unwrap();
        //
        // let response = client
        //     .get(&keypair.public_key(), "/pub/foo.txt")
        //     .await
        //     .unwrap();
        //
        // assert_eq!(response, None);
    }

    #[tokio::test]
    async fn forbidden_put_delete() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let public_key = keypair.public_key();

        let other_client = PubkyClient::test(&testnet);
        {
            let other = Keypair::random();

            other_client
                .signup(&other, &server.public_key())
                .await
                .unwrap();

            let response = other_client
                .put(&public_key, "/pub/foo.txt", &[0, 1, 2, 3, 4])
                .await;

            match response {
                Err(Error::Reqwest(error)) => {
                    assert!(error.status() == Some(StatusCode::UNAUTHORIZED))
                }
                error => {
                    panic!("expected error StatusCode::UNAUTHORIZED")
                }
            }
        }

        // client
        //     .put(&keypair.public_key(), "/pub/foo.txt", &[0, 1, 2, 3, 4])
        //     .await
        //     .unwrap();
        //
        // client
        // .delete(&keypair.public_key(), "/pub/foo.txt")
        //     .await
        //     .unwrap();
        //
        // let response = client
        //     .get(&keypair.public_key(), "/pub/foo.txt")
        //     .await
        //     .unwrap();
        //
        // assert_eq!(response, None);
    }
}
