#![allow(unused)]

mod client;
mod client_async;
mod error;

use client::PubkyClient;

#[cfg(test)]
mod tests {
    use super::*;

    use pkarr::{mainline::Testnet, Keypair};
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
    }
}
