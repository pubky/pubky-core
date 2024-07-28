use reqwest::StatusCode;

use pkarr::{Keypair, PublicKey};
use pubky_common::{auth::AuthnSignature, session::Session};

use crate::{
    error::{Error, Result},
    PubkyClient,
};

impl PubkyClient {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub async fn signup(&self, keypair: &Keypair, homeserver: &PublicKey) -> Result<()> {
        let homeserver = homeserver.to_string();

        let (audience, mut url) = self.resolve_endpoint(&homeserver).await?;

        url.set_path(&format!("/{}", keypair.public_key()));

        let body = AuthnSignature::generate(keypair, &audience)
            .as_bytes()
            .to_owned();

        self.http.put(url).body(body).send().await?;

        self.publish_pubky_homeserver(keypair, &homeserver).await?;

        Ok(())
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns an [Error::NotSignedIn] if so, or [ureq::Error] if
    /// the response has any other `>=400` status code.
    pub async fn session(&self, pubky: &PublicKey) -> Result<Session> {
        let (homeserver, mut url) = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{}/session", pubky));

        let res = self.http.get(url).send().await?;

        if res.status() == StatusCode::NOT_FOUND {
            return Err(Error::NotSignedIn);
        }

        if !res.status().is_success() {
            res.error_for_status_ref()?;
        };

        let bytes = res.bytes().await?;

        Ok(Session::deserialize(&bytes)?)
    }

    /// Signout from a homeserver.
    pub async fn signout(&self, pubky: &PublicKey) -> Result<()> {
        let (homeserver, mut url) = self.resolve_pubky_homeserver(pubky).await?;

        url.set_path(&format!("/{}/session", pubky));

        self.http.delete(url).send().await?;

        Ok(())
    }

    /// Signin to a homeserver.
    pub async fn signin(&self, keypair: &Keypair) -> Result<()> {
        let pubky = keypair.public_key();

        let (audience, mut url) = self.resolve_pubky_homeserver(&pubky).await?;

        url.set_path(&format!("/{}/session", &pubky));

        let body = AuthnSignature::generate(keypair, &audience)
            .as_bytes()
            .to_owned();

        self.http.post(url).body(body).send().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    use pkarr::{mainline::Testnet, Keypair};
    use pubky_common::session::Session;
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn basic_authn() {
        let testnet = Testnet::new(3);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = PubkyClient::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let session = client.session(&keypair.public_key()).await.unwrap();

        assert_eq!(session, Session { ..session.clone() });

        client.signout(&keypair.public_key()).await.unwrap();

        {
            let session = client.session(&keypair.public_key()).await;

            assert!(session.is_err());

            match session {
                Err(Error::NotSignedIn) => {}
                _ => panic!("expected NotSignedInt error"),
            }
        }

        client.signin(&keypair).await.unwrap();

        {
            let session = client.session(&keypair.public_key()).await.unwrap();

            assert_eq!(session, Session { ..session.clone() });
        }
    }
}
