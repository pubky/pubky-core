use reqwest::{Method, StatusCode};

use pkarr::{Keypair, PublicKey};
use pubky_common::{auth::AuthnSignature, session::Session};
use url::Url;

use crate::{
    error::{Error, Result},
    PubkyClient,
};

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

        let public_key = &keypair.public_key();

        let Endpoint {
            public_key: audience,
            mut url,
        } = self.resolve_endpoint(&homeserver).await?;

        url.set_path(&format!("/{}", public_key));

        let body = AuthnSignature::generate(keypair, &audience)
            .as_bytes()
            .to_owned();

        let response = self
            .request(Method::PUT, url.clone())
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
        let Endpoint {
            public_key,
            mut url,
        } = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{}/session", pubky));

        self.request(Method::DELETE, url).send().await?;

        self.remove_session(pubky);

        Ok(())
    }

    /// Signin to a homeserver.
    pub(crate) async fn inner_signin(&self, keypair: &Keypair) -> Result<()> {
        let pubky = keypair.public_key();

        let Endpoint {
            public_key: audience,
            mut url,
        } = self.resolve_pubky_homeserver(&pubky).await?;

        url.set_path(&format!("/{}/session", &pubky));

        let body = AuthnSignature::generate(keypair, &audience)
            .as_bytes()
            .to_owned();

        let response = self.request(Method::POST, url).body(body).send().await?;

        self.store_session(response);

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use std::time::Duration;

    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_common::session::Session;
    use pubky_homeserver::Homeserver;
    use tokio::time::sleep;

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

        assert_eq!(session, Session { ..session.clone() });

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

            assert_eq!(session, Session { ..session.clone() });
        }
    }
}
