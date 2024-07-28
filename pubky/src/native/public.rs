use bytes::Bytes;

use pkarr::PublicKey;

use crate::{error::Result, PubkyClient};

impl PubkyClient {
    pub async fn put(&self, pubky: &PublicKey, path: &str, content: &[u8]) -> Result<()> {
        let path = normalize_path(path);

        let (_, mut url) = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{pubky}/{path}"));

        self.http.put(url).body(content.to_owned()).send().await?;

        Ok(())
    }

    pub async fn get(&self, pubky: &PublicKey, path: &str) -> Result<Bytes> {
        let path = normalize_path(path);

        let (_, mut url) = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{pubky}/{path}"));

        let response = self.http.get(url).send().await?;

        // TODO: bail on too large files.
        let bytes = response.bytes().await?;

        Ok(bytes)
    }
}

fn normalize_path(path: &str) -> String {
    let mut path = path.to_string();

    if path.starts_with('/') {
        path = path[1..].to_string()
    }

    // TODO: should we return error instead?
    if path.ends_with('/') {
        path = path[..path.len()].to_string()
    }

    path
}

#[cfg(test)]
mod tests {

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn put_get() {
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
            .unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]))
    }
}
