use reqwest::{Method, StatusCode};

use base64::{alphabet::URL_SAFE, engine::general_purpose::NO_PAD, Engine};
use pkarr::{Keypair, PublicKey};
use pubky_common::{
    auth::AuthToken,
    capabilities::Capability,
    crypto::{decrypt, encrypt, hash},
    session::Session,
};
use url::Url;

use crate::{error::Result, PubkyClient};

use super::pkarr::Endpoint;

impl PubkyClient {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub(crate) async fn inner_signup(
        &self,
        keypair: &Keypair,
        homeserver: &PublicKey,
    ) -> Result<()> {
        let homeserver = homeserver.to_string();

        let Endpoint { mut url, .. } = self.resolve_endpoint(&homeserver).await?;

        url.set_path("/signup");

        let body = AuthToken::sign(keypair, vec![Capability::root()]).serialize();

        let response = self
            .request(Method::POST, url.clone())
            .body(body)
            .send()
            .await?;

        self.store_session(response);

        self.publish_pubky_homeserver(keypair, &homeserver).await?;

        Ok(())
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns None  if not signed in, or [reqwest::Error]
    /// if the response has any other `>=404` status code.
    pub(crate) async fn inner_session(&self, pubky: &PublicKey) -> Result<Option<Session>> {
        let Endpoint { mut url, .. } = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{}/session", pubky));

        let res = self.request(Method::GET, url).send().await?;

        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !res.status().is_success() {
            res.error_for_status_ref()?;
        };

        let bytes = res.bytes().await?;

        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Signout from a homeserver.
    pub(crate) async fn inner_signout(&self, pubky: &PublicKey) -> Result<()> {
        let Endpoint { mut url, .. } = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{}/session", pubky));

        self.request(Method::DELETE, url).send().await?;

        self.remove_session(pubky);

        Ok(())
    }

    /// Signin to a homeserver.
    pub(crate) async fn inner_signin(&self, keypair: &Keypair) -> Result<()> {
        let pubky = keypair.public_key();

        let Endpoint { mut url, .. } = self.resolve_pubky_homeserver(&pubky).await?;

        url.set_path("/session");

        let token = AuthToken::sign(keypair, vec![Capability::root()]);

        let response = self
            .request(Method::POST, url)
            .body(token.serialize())
            .send()
            .await?;

        self.store_session(response);

        Ok(())
    }

    pub async fn authorize(
        &self,
        keypair: &Keypair,
        capabilities: Vec<Capability>,
        client_secret: [u8; 32],
        relay: &Url,
    ) -> Result<()> {
        let token = AuthToken::sign(keypair, capabilities);

        let encrypted_token = encrypt(&token.serialize(), &client_secret)?;

        let channel_id = hash(&client_secret);
        let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);
        let channel_id = engine.encode(channel_id.as_bytes());

        let mut callback = relay.clone();
        let mut path_segments = callback.path_segments_mut().unwrap();
        path_segments.push(&channel_id);

        drop(path_segments);

        self.request(Method::POST, callback)
            .body(encrypted_token)
            .send()
            .await?;

        Ok(())
    }

    pub async fn inner_third_party_signin(
        &self,
        encrypted_token: &[u8],
        client_secret: &[u8; 32],
    ) -> Result<PublicKey> {
        let decrypted = decrypt(encrypted_token, client_secret)?;
        let token = AuthToken::deserialize(&decrypted)?;

        let pubky = token.pubky();

        let Endpoint { mut url, .. } = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path("/session");

        let response = self
            .request(Method::POST, url)
            .body(token.serialize())
            .send()
            .await?;

        self.store_session(response);

        Ok(pubky.to_owned())
    }
}

#[cfg(test)]
mod tests {

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_common::capabilities::Capability;
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn basic_authn() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let session = client
            .session(&keypair.public_key())
            .await
            .unwrap()
            .unwrap();

        assert!(session.capabilities.contains(&Capability::root()));

        client.signout(&keypair.public_key()).await.unwrap();

        {
            let session = client.session(&keypair.public_key()).await.unwrap();

            assert!(session.is_none());
        }

        client.signin(&keypair).await.unwrap();

        {
            let session = client
                .session(&keypair.public_key())
                .await
                .unwrap()
                .unwrap();

            assert!(session.capabilities.contains(&Capability::root()));
        }
    }
}
