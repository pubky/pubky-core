#![allow(unused)]

mod client;
mod client_async;
mod error;

pub use client::PubkyClient;
pub use error::Error;

#[cfg(test)]
mod tests {
    use super::*;

    use super::error::Error;

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
