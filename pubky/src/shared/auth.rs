use std::collections::HashMap;

use base64::{alphabet::URL_SAFE, engine::general_purpose::NO_PAD, Engine};
use reqwest::{IntoUrl, Method, StatusCode};
use url::Url;

use pkarr::{Keypair, PublicKey};
use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    crypto::{decrypt, encrypt, hash, random_bytes},
    session::Session,
};

use anyhow::Result;

use crate::{handle_http_error, Client};

impl Client {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub(crate) async fn inner_signup(
        &self,
        keypair: &Keypair,
        homeserver: &PublicKey,
    ) -> Result<Session> {
        let response = self
            .inner_request(Method::POST, format!("https://{}/signup", homeserver))
            .await
            .body(AuthToken::sign(keypair, vec![Capability::root()]).serialize())
            .send()
            .await?;

        handle_http_error!(response);

        self.publish_homeserver(keypair, &homeserver.to_string())
            .await?;

        // Store the cookie to the correct URL.
        #[cfg(not(target_arch = "wasm32"))]
        self.cookie_store
            .store_session_after_signup(&response, &keypair.public_key());

        let bytes = response.bytes().await?;

        Ok(Session::deserialize(&bytes)?)
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns None  if not signed in, or [reqwest::Error]
    /// if the response has any other `>=404` status code.
    pub(crate) async fn inner_session(&self, pubky: &PublicKey) -> Result<Option<Session>> {
        let response = self
            .inner_request(Method::GET, format!("pubky://{}/session", pubky))
            .await
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        handle_http_error!(response);

        let bytes = response.bytes().await?;

        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Signout from a homeserver.
    pub(crate) async fn inner_signout(&self, pubky: &PublicKey) -> Result<()> {
        let response = self
            .inner_request(Method::DELETE, format!("pubky://{}/session", pubky))
            .await
            .send()
            .await?;

        handle_http_error!(response);

        #[cfg(not(target_arch = "wasm32"))]
        self.cookie_store.delete_session_after_signout(pubky);

        Ok(())
    }

    /// Signin to a homeserver.
    pub(crate) async fn inner_signin(&self, keypair: &Keypair) -> Result<Session> {
        let token = AuthToken::sign(keypair, vec![Capability::root()]);

        self.signin_with_authtoken(&token).await
    }

    pub(crate) async fn inner_send_auth_token<T: IntoUrl>(
        &self,
        keypair: &Keypair,
        pubkyauth_url: T,
    ) -> Result<()> {
        let pubkyauth_url = Url::parse(
            pubkyauth_url
                .as_str()
                .replace("pubkyauth_url", "http")
                .as_str(),
        )?;

        let query_params: HashMap<String, String> =
            pubkyauth_url.query_pairs().into_owned().collect();

        let relay = query_params
            .get("relay")
            .map(|r| url::Url::parse(r).expect("Relay query param to be valid URL"))
            .expect("Missing relay query param");

        let client_secret = query_params
            .get("secret")
            .map(|s| {
                let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);
                let bytes = engine.decode(s).expect("invalid client_secret");
                let arr: [u8; 32] = bytes.try_into().expect("invalid client_secret");

                arr
            })
            .expect("Missing client secret");

        let capabilities = query_params
            .get("caps")
            .map(|caps_string| {
                caps_string
                    .split(',')
                    .filter_map(|cap| Capability::try_from(cap).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let token = AuthToken::sign(keypair, capabilities);

        let encrypted_token = encrypt(&token.serialize(), &client_secret)?;

        let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);

        let mut callback_url = relay.clone();
        let mut path_segments = callback_url.path_segments_mut().unwrap();
        path_segments.pop_if_empty();
        let channel_id = engine.encode(hash(&client_secret).as_bytes());
        path_segments.push(&channel_id);
        drop(path_segments);

        let response = self
            .inner_request(Method::POST, callback_url)
            .await
            .body(encrypted_token)
            .send()
            .await?;

        handle_http_error!(response);

        Ok(())
    }

    pub(crate) async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
        let response = self
            .inner_request(Method::POST, format!("pubky://{}/session", token.pubky()))
            .await
            .body(token.serialize())
            .send()
            .await?;

        handle_http_error!(response);

        let bytes = response.bytes().await?;

        Ok(Session::deserialize(&bytes)?)
    }

    pub(crate) fn create_auth_request(
        &self,
        relay: &mut Url,
        capabilities: &Capabilities,
    ) -> Result<(Url, [u8; 32])> {
        let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);

        let client_secret: [u8; 32] = random_bytes::<32>();

        let pubkyauth_url = Url::parse(&format!(
            "pubkyauth:///?caps={capabilities}&secret={}&relay={relay}",
            engine.encode(client_secret)
        ))?;

        let mut segments = relay
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid relay"))?;

        // remove trailing slash if any.
        segments.pop_if_empty();
        let channel_id = &engine.encode(hash(&client_secret).as_bytes());
        segments.push(channel_id);
        drop(segments);

        Ok((pubkyauth_url, client_secret))
    }

    pub(crate) async fn subscribe_to_auth_response(
        &self,
        relay: Url,
        client_secret: &[u8; 32],
    ) -> Result<PublicKey> {
        // TODO: use a clearnet client.
        let response = reqwest::get(relay).await?;
        let encrypted_token = response.bytes().await?;
        let token_bytes = decrypt(&encrypted_token, client_secret)
            .map_err(|e| anyhow::anyhow!("Got invalid token: {e}"))?;
        let token = AuthToken::verify(&token_bytes)?;

        if !token.capabilities().is_empty() {
            self.signin_with_authtoken(&token).await?;
        }

        Ok(token.pubky().clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    use http_relay::HttpRelay;
    use pkarr::{mainline::Testnet, Keypair};
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_homeserver::Homeserver;
    use reqwest::StatusCode;

    #[tokio::test]
    async fn basic_authn() {
        let testnet = Testnet::new(10).unwrap();
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let client = Client::test(&testnet);

        let keypair = Keypair::random();

        client.signup(&keypair, &server.public_key()).await.unwrap();

        let session = client
            .session(&keypair.public_key())
            .await
            .unwrap()
            .unwrap();

        assert!(session.capabilities().contains(&Capability::root()));

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

            assert_eq!(session.pubky(), &keypair.public_key());
            assert!(session.capabilities().contains(&Capability::root()));
        }
    }

    #[tokio::test]
    async fn authz() {
        let testnet = Testnet::new(10).unwrap();
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let http_relay = HttpRelay::builder().build().await.unwrap();
        let http_relay_url = http_relay.local_link_url();

        let keypair = Keypair::random();
        let pubky = keypair.public_key();

        // Third party app side
        let capabilities: Capabilities =
            "/pub/pubky.app/:rw,/pub/foo.bar/file:r".try_into().unwrap();
        let client = Client::test(&testnet);

        let (pubkyauth_url, pubkyauth_response) =
            client.auth_request(http_relay_url, &capabilities).unwrap();

        // Authenticator side
        {
            let client = Client::test(&testnet);

            client.signup(&keypair, &server.public_key()).await.unwrap();

            client
                .send_auth_token(&keypair, pubkyauth_url)
                .await
                .unwrap();
        }

        let public_key = pubkyauth_response
            .await
            .expect("sender to not be dropped")
            .unwrap();

        assert_eq!(&public_key, &pubky);

        let session = client.session(&pubky).await.unwrap().unwrap();
        assert_eq!(session.capabilities(), &capabilities.0);

        // Test access control enforcement

        client
            .put(format!("pubky://{pubky}/pub/pubky.app/foo"))
            .body(vec![])
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        assert_eq!(
            client
                .put(format!("pubky://{pubky}/pub/pubky.app"))
                .body(vec![])
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );

        assert_eq!(
            client
                .put(format!("pubky://{pubky}/pub/foo.bar/file"))
                .body(vec![])
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
