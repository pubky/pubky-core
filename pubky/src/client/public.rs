use bytes::Bytes;

use pkarr::PublicKey;

use super::{PubkyClient, Result};

impl PubkyClient {
    pub fn put(&self, pubky: &PublicKey, path: &str, content: &[u8]) -> Result<()> {
        let path = normalize_path(path);

        let (_, mut url) = self.resolve_pubky_homeserver(pubky)?;

        url.set_path(&format!("/{pubky}/{path}"));

        self.request(super::HttpMethod::Put, &url)
            .send_bytes(content)?;

        Ok(())
    }

    pub fn get(&self, pubky: &PublicKey, path: &str) -> Result<Bytes> {
        let path = normalize_path(path);

        let (_, mut url) = self.resolve_pubky_homeserver(pubky)?;

        url.set_path(&format!("/{pubky}/{path}"));

        let response = self.request(super::HttpMethod::Get, &url).call()?;

        let len = response
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok())
            // TODO: return an error in case content-length header is missing
            .unwrap_or(0);

        // TODO: bail on too large files.

        let mut bytes = vec![0; len as usize];

        response.into_reader().read_exact(&mut bytes);

        Ok(bytes.into())
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
    use std::ops::Deref;

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_common::session::Session;
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn put_get() {
        let testnet = Testnet::new(3);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client
            .signup(&keypair, &server.public_key().to_string())
            .await
            .unwrap();

        let response = client
            .put(&keypair.public_key(), "/pub/foo.txt", &[0, 1, 2, 3, 4])
            .await;

        if let Err(Error::Ureq(ureqerror)) = response {
            if let Some(r) = ureqerror.into_response() {
                dbg!(r.into_string());
            }
        }

        let response = client
            .get(&keypair.public_key(), "/pub/foo.txt")
            .await
            .unwrap();

        assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]))
    }
}
