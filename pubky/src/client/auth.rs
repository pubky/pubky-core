use pkarr::{Keypair, PublicKey};

use pubky_common::{auth::AuthnSignature, session::Session};

use super::{Error, HttpMethod, PubkyClient, Result};

impl PubkyClient {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub fn signup(&self, keypair: &Keypair, homeserver: &str) -> Result<()> {
        let (audience, mut url) = self.resolve_endpoint(homeserver)?;

        url.set_path(&format!("/{}", keypair.public_key()));

        self.request(HttpMethod::Put, &url)
            .send_bytes(AuthnSignature::generate(keypair, &audience).as_bytes())?;

        self.publish_pubky_homeserver(keypair, homeserver);

        Ok(())
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns an [Error::NotSignedIn] if so, or [ureq::Error] if
    /// the response has any other `>=400` status code.
    pub fn session(&self, pubky: &PublicKey) -> Result<Session> {
        let (homeserver, mut url) = self.resolve_pubky_homeserver(pubky)?;

        url.set_path(&format!("/{}/session", pubky));

        let mut bytes = vec![];

        let result = self.request(HttpMethod::Get, &url).call().map_err(Box::new);

        let reader = self.request(HttpMethod::Get, &url).call().map_err(|err| {
            match err {
                ureq::Error::Status(404, _) => Error::NotSignedIn,
                // TODO: handle other types of errors
                _ => err.into(),
            }
        })?;

        reader.into_reader().read_to_end(&mut bytes);

        Ok(Session::deserialize(&bytes)?)
    }

    /// Signout from a homeserver.
    pub fn signout(&self, pubky: &PublicKey) -> Result<()> {
        let (homeserver, mut url) = self.resolve_pubky_homeserver(pubky)?;

        url.set_path(&format!("/{}/session", pubky));

        self.request(HttpMethod::Delete, &url)
            .call()
            .map_err(Box::new)?;

        Ok(())
    }

    /// Signin to a homeserver.
    pub fn signin(&self, keypair: &Keypair) -> Result<()> {
        let pubky = keypair.public_key();

        let (audience, mut url) = self.resolve_pubky_homeserver(&pubky)?;

        url.set_path(&format!("/{}/session", &pubky));

        self.request(HttpMethod::Post, &url)
            .send_bytes(AuthnSignature::generate(keypair, &audience).as_bytes())
            .map_err(Box::new)?;

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

        let client = PubkyClient::test(&testnet).as_async();

        let keypair = Keypair::random();

        client
            .signup(&keypair, &server.public_key().to_string())
            .await
            .unwrap();

        let session = client.session(&keypair.public_key()).await.unwrap();

        assert_eq!(session, Session { ..session.clone() });

        client.signout(&keypair.public_key()).await.unwrap();

        {
            let session = client.session(&keypair.public_key()).await;

            assert!(session.is_err());

            match session {
                Err(Error::NotSignedIn) => {}
                _ => assert!(false, "expected NotSignedInt error"),
            }
        }

        client.signin(&keypair).await.unwrap();

        {
            let session = client.session(&keypair.public_key()).await.unwrap();

            assert_eq!(session, Session { ..session.clone() });
        }
    }
}
