use pkarr::Keypair;
use pubky_common::session::Session;
use tokio::sync::oneshot;
use url::Url;

use pkarr::PublicKey;

use pubky_common::capabilities::Capabilities;

use crate::error::{Error, Result};
use crate::PubkyClient;

impl PubkyClient {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub async fn signup(&self, keypair: &Keypair, homeserver: &PublicKey) -> Result<Session> {
        self.inner_signup(keypair, homeserver).await
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns [Session] or `None` (if recieved `404 NOT_FOUND`),
    /// or [reqwest::Error] if the response has any other `>=400` status code.
    pub async fn session(&self, pubky: &PublicKey) -> Result<Option<Session>> {
        self.inner_session(pubky).await
    }

    /// Signout from a homeserver.
    pub async fn signout(&self, pubky: &PublicKey) -> Result<()> {
        self.inner_signout(pubky).await
    }

    /// Signin to a homeserver.
    pub async fn signin(&self, keypair: &Keypair) -> Result<Session> {
        self.inner_signin(keypair).await
    }

    /// Return `pubkyauth://` url and wait for the incoming [AuthToken]
    /// verifying that AuthToken, and if capabilities were requested, signing in to
    /// the Pubky's homeserver and returning the [Session] information.
    pub fn auth_request(
        &self,
        relay: impl TryInto<Url>,
        capabilities: &Capabilities,
    ) -> Result<(Url, tokio::sync::oneshot::Receiver<PublicKey>)> {
        let mut relay: Url = relay
            .try_into()
            .map_err(|_| Error::Generic("Invalid relay Url".into()))?;

        let (pubkyauth_url, client_secret) = self.create_auth_request(&mut relay, capabilities)?;

        let (tx, rx) = oneshot::channel::<PublicKey>();

        let this = self.clone();

        tokio::spawn(async move {
            let to_send = this
                .subscribe_to_auth_response(relay, &client_secret)
                .await?;

            tx.send(to_send)
                .map_err(|_| Error::Generic("Failed to send the session after signing in with token, since the receiver is dropped".into()))?;

            Ok::<(), Error>(())
        });

        Ok((pubkyauth_url, rx))
    }

    /// Sign an [pubky_common::auth::AuthToken], encrypt it and send it to the
    /// source of the pubkyauth request url.
    pub async fn send_auth_token<T: TryInto<Url>>(
        &self,
        keypair: &Keypair,
        pubkyauth_url: T,
    ) -> Result<()> {
        let url: Url = pubkyauth_url.try_into().map_err(|_| Error::InvalidUrl)?;

        self.inner_send_auth_token(keypair, url).await?;

        Ok(())
    }
}
