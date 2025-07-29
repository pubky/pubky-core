use anyhow::Result;
use pkarr::PublicKey;

use crate::{BaseClient, Client};

impl Client {
    /// Signs out from a homeserver and clears the local session cookie.
    ///
    /// This method wraps the generic signout logic and adds the native-specific
    /// action of explicitly deleting the session cookie from the custom `CookieJar`.
    pub async fn signout_and_clear_session(&self, pubky: &PublicKey) -> Result<()> {
        // First, call the generic signout method to perform the HTTP DELETE request.
        BaseClient::signout(self, pubky).await?;

        // After the request succeeds, explicitly delete the cookie from the native store.
        self.http.cookie_store.delete_session_after_signout(pubky);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Client, internal::pkarr::PublishStrategy};
    use pkarr::Keypair;

    /// Test the native implementation for get_homeserver in an e2e way
    #[tokio::test]
    async fn test_get_homeserver() {
        let dht = mainline::Testnet::new(3).unwrap();
        let mut config = Client::config();
        config.pkarr(|builder| builder.bootstrap(&dht.bootstrap));

        let client = Client::from_config(config).unwrap();
        let keypair = Keypair::random();
        let pubky = keypair.public_key();

        let homeserver_key = Keypair::random().public_key().to_z32();
        client
            .publish_homeserver(
                &keypair,
                Some(homeserver_key.as_str()),
                PublishStrategy::Force,
            )
            .await
            .unwrap();
        let homeserver = client.get_homeserver(&pubky).await;
        assert_eq!(homeserver, Some(homeserver_key));
    }
}
