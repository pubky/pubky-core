use bytes::Bytes;

use pkarr::PublicKey;
use reqwest::{Method, Response, StatusCode};
use url::Url;

use crate::{error::Result, PubkyClient};

impl PubkyClient {
    pub async fn inner_put(&self, pubky: &PublicKey, path: &str, content: &[u8]) -> Result<()> {
        let url = self.url(pubky, path).await?;

        self.request(Method::PUT, url)
            .body(content.to_owned())
            .send()
            .await?;

        Ok(())
    }

    pub async fn inner_get(&self, pubky: &PublicKey, path: &str) -> Result<Option<Bytes>> {
        let url = self.url(pubky, path).await?;

        let res = self.request(Method::GET, url).send().await?;

        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        // TODO: bail on too large files.
        let bytes = res.bytes().await?;

        Ok(Some(bytes))
    }

    pub async fn inner_delete(&self, pubky: &PublicKey, path: &str) -> Result<()> {
        let url = self.url(pubky, path).await?;

        self.request(Method::DELETE, url).send().await?;

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

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn put_get_delete() {
        let testnet = Testnet::new(3);
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
}
