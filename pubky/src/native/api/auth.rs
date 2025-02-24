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

use crate::{handle_http_error, native::internal::pkarr::PublishStrategy};

use super::super::Client;

impl Client {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub async fn signup(&self, keypair: &Keypair, homeserver: &PublicKey) -> Result<Session> {
        let response = self
            .cross_request(Method::POST, format!("https://{}/signup", homeserver))
            .await
            .body(AuthToken::sign(keypair, vec![Capability::root()]).serialize())
            .send()
            .await?;

        handle_http_error!(response);

        // Publish homeserver Pkarr record for the first time (force)
        self.publish_homeserver(
            keypair,
            Some(&homeserver.to_string()),
            PublishStrategy::Force,
        )
        .await?;

        // Store the cookie to the correct URL.
        #[cfg(not(target_arch = "wasm32"))]
        self.cookie_store
            .store_session_after_signup(&response, &keypair.public_key());

        let bytes = response.bytes().await?;

        Ok(Session::deserialize(&bytes)?)
    }

    /// Check the current session for a given Pubky in its homeserver.
    ///
    /// Returns None  if not signed in, or [reqwest::Error]
    /// if the response has any other `>=404` status code.
    pub async fn session(&self, pubky: &PublicKey) -> Result<Option<Session>> {
        let response = self
            .cross_request(Method::GET, format!("pubky://{}/session", pubky))
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
    pub async fn signout(&self, pubky: &PublicKey) -> Result<()> {
        let response = self
            .cross_request(Method::DELETE, format!("pubky://{}/session", pubky))
            .await
            .send()
            .await?;

        handle_http_error!(response);

        #[cfg(not(target_arch = "wasm32"))]
        self.cookie_store.delete_session_after_signout(pubky);

        Ok(())
    }

    /// Signin to a homeserver.
    /// After a successful signin, a background task is spawned to republish the user's
    /// PKarr record if it is missing or older than 4 days. We don't mind if it succeed
    /// or fails. We want signin to return fast.
    pub async fn signin(&self, keypair: &Keypair) -> Result<Session> {
        let token = AuthToken::sign(keypair, vec![Capability::root()]);
        let session = self.signin_with_authtoken(&token).await?;

        // Spawn a background task to republish the record.
        let client_clone = self.clone();
        let keypair_clone = keypair.clone();
        tokio::spawn(async move {
            // Resolve the record and republish if existing and older MAX_HOMESERVER_RECORD_AGE_SECS
            let _ = client_clone
                .publish_homeserver(&keypair_clone, None, PublishStrategy::IfOlderThan)
                .await;
        });

        Ok(session)
    }

    pub async fn send_auth_token<T: IntoUrl>(
        &self,
        keypair: &Keypair,
        pubkyauth_url: &T,
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

        let encrypted_token = encrypt(&token.serialize(), &client_secret);

        let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);

        let mut callback_url = relay.clone();
        let mut path_segments = callback_url.path_segments_mut().unwrap();
        path_segments.pop_if_empty();
        let channel_id = engine.encode(hash(&client_secret).as_bytes());
        path_segments.push(&channel_id);
        drop(path_segments);

        let response = self
            .cross_request(Method::POST, callback_url)
            .await
            .body(encrypted_token)
            .send()
            .await?;

        handle_http_error!(response);

        Ok(())
    }

    pub(crate) async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
        let response = self
            .cross_request(Method::POST, format!("pubky://{}/session", token.pubky()))
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

    /// Return `pubkyauth://` url and wait for the incoming [AuthToken]
    /// verifying that AuthToken, and if capabilities were requested, signing in to
    /// the Pubky's homeserver and returning the [Session] information.
    pub fn auth_request<T: IntoUrl>(
        &self,
        relay: T,
        capabilities: &Capabilities,
    ) -> Result<AuthRequest> {
        // TODO: use `async_compat` to remove the dependency on Tokio runtime.
        let mut relay: Url = relay.into_url()?;

        let (url, client_secret) = self.create_auth_request(&mut relay, capabilities)?;

        let (tx, rx) = flume::bounded(1);

        let this = self.clone();

        let future = async move {
            let result = this
                .subscribe_to_auth_response(relay, &client_secret, tx.clone())
                .await;
            let _ = tx.send(result);
        };

        #[cfg(not(wasm_browser))]
        tokio::spawn(future);
        #[cfg(wasm_browser)]
        wasm_bindgen_futures::spawn_local(future);

        Ok(AuthRequest { url, rx })
    }
    pub(crate) async fn subscribe_to_auth_response(
        &self,
        relay: Url,
        client_secret: &[u8; 32],
        tx: flume::Sender<Result<PublicKey>>,
    ) -> anyhow::Result<PublicKey> {
        let response = loop {
            match self
                .cross_request(Method::GET, relay.clone())
                .await
                .send()
                .await
            {
                Ok(response) => {
                    break Ok(response);
                }
                Err(error) => {
                    // TODO: test again after Rqewest support timeout
                    if error.is_timeout() && !tx.is_disconnected() {
                        cross_debug!("Connection to HttpRelay timedout, reconnecting...");

                        continue;
                    }

                    break Err(error);
                }
            }
        }?;

        let encrypted_token = response.bytes().await?;
        let token_bytes = decrypt(&encrypted_token, client_secret)
            .map_err(|e| anyhow::anyhow!("Got invalid token: {e}"))?;
        let token = AuthToken::verify(&token_bytes)?;

        if !token.capabilities().is_empty() {
            self.signin_with_authtoken(&token).await?;
        }

        Ok(token.pubky().clone())
    }

    /// Republish the user's Pkarr record pointing to their homeserver if
    /// no record can be resolved or if the existing record is older than 4 days.
    ///
    /// This method is intended to be used by clients and key managers (e.g., pubky-ring)
    /// in order to keep the records of active users fresh and available in the DHT.
    /// It is intended to be used only after failed signin due to homeserver
    /// resolution failure. This method is lighter than performing a re-signup into
    /// the last known homeserver, but does not return a session token, so a signin
    /// must be done after republishing if a session token is needed. On a failed
    /// signin due to homeserver resolution failure, `pubky-ring` should always
    /// republish the last known homeserver.
    ///
    /// # Arguments
    ///
    /// * `keypair` - The keypair associated with the record.
    /// * `host` - The homeserver to publish the record for.
    ///
    /// # Errors
    ///
    /// Returns an error if the publication fails.
    pub async fn republish_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        self.publish_homeserver(keypair, Some(host), PublishStrategy::IfOlderThan)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct AuthRequest {
    url: Url,
    pub(crate) rx: flume::Receiver<Result<PublicKey>>,
}

impl AuthRequest {
    /// Returns the Pubky Auth URL.
    pub fn url(&self) -> &Url {
        &self.url
    }

    // TODO: Return better errors

    /// Returns the result of an Auth request.
    pub async fn response(&self) -> Result<PublicKey> {
        self.rx
            .recv_async()
            .await
            .expect("sender dropped unexpectedly")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pkarr::Keypair;
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_testnet::Testnet;
    use reqwest::StatusCode;

    #[tokio::test]
    async fn basic_authn() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

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
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let http_relay = testnet.run_http_relay().await.unwrap();
        let http_relay_url = http_relay.local_link_url();

        let keypair = Keypair::random();
        let pubky = keypair.public_key();

        // Third party app side
        let capabilities: Capabilities =
            "/pub/pubky.app/:rw,/pub/foo.bar/file:r".try_into().unwrap();

        let client = testnet.client_builder().build().unwrap();

        let pubky_auth_request = client.auth_request(http_relay_url, &capabilities).unwrap();

        // Authenticator side
        {
            let client = testnet.client_builder().build().unwrap();

            client.signup(&keypair, &server.public_key()).await.unwrap();

            client
                .send_auth_token(&keypair, pubky_auth_request.url())
                .await
                .unwrap();
        }

        let public_key = pubky_auth_request.response().await.unwrap();

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

    #[tokio::test]
    async fn multiple_users() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let first_keypair = Keypair::random();
        let second_keypair = Keypair::random();

        client
            .signup(&first_keypair, &server.public_key())
            .await
            .unwrap();

        client
            .signup(&second_keypair, &server.public_key())
            .await
            .unwrap();

        let session = client
            .session(&first_keypair.public_key())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(session.pubky(), &first_keypair.public_key());
        assert!(session.capabilities().contains(&Capability::root()));

        let session = client
            .session(&second_keypair.public_key())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(session.pubky(), &second_keypair.public_key());
        assert!(session.capabilities().contains(&Capability::root()));
    }

    #[tokio::test]
    async fn authz_timeout_reconnect() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let http_relay = testnet.run_http_relay().await.unwrap();
        let http_relay_url = http_relay.local_link_url();

        let keypair = Keypair::random();
        let pubky = keypair.public_key();

        // Third party app side
        let capabilities: Capabilities =
            "/pub/pubky.app/:rw,/pub/foo.bar/file:r".try_into().unwrap();

        let client = testnet
            .client_builder()
            .request_timeout(Duration::from_millis(1000))
            .build()
            .unwrap();

        let pubky_auth_request = client.auth_request(http_relay_url, &capabilities).unwrap();

        // Authenticator side
        {
            let url = pubky_auth_request.url().clone();

            let client = testnet.client_builder().build().unwrap();
            client.signup(&keypair, &server.public_key()).await.unwrap();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(400)).await;
                // loop {
                client.send_auth_token(&keypair, &url).await.unwrap();
                //     }
            });
        }

        let public_key = pubky_auth_request.response().await.unwrap();

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

    // This test verifies that when a signin happens immediately after signup,
    // the record is not republished on signin (its timestamp remains unchanged)
    // but when a signin happens after the record is “old” (in test, after 1 second),
    // the record is republished (its timestamp increases).
    #[tokio::test]
    async fn test_republish_on_signin() {
        // Setup the testnet and run a homeserver.
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();
        // Create a client that will republish conditionally if a record is older than 1 second
        let client = testnet
            .client_builder()
            .max_record_age_micros(1_000_000)
            .build()
            .unwrap();
        let keypair = Keypair::random();

        // Signup publishes a new record.
        client.signup(&keypair, &server.public_key()).await.unwrap();
        // Resolve the record and get its timestamp.
        let record1 = client
            .pkarr()
            .resolve_most_recent(&keypair.public_key())
            .await
            .unwrap();
        let ts1 = record1.timestamp().as_u64();

        // Immediately sign in. This spawns a background task to update the record
        // with PublishStrategy::IfOlderThan.
        client.signin(&keypair).await.unwrap();
        // Wait a short time to let the background task complete.
        tokio::time::sleep(Duration::from_millis(5)).await;
        let record2 = client
            .pkarr()
            .resolve_most_recent(&keypair.public_key())
            .await
            .unwrap();
        let ts2 = record2.timestamp().as_u64();

        // Because the record is fresh (less than 1 second old in our test configuration),
        // the background task should not republish it. The timestamp should remain the same.
        assert_eq!(
            ts1, ts2,
            "Record republished too early; timestamps should be equal"
        );

        // Wait long enough for the record to be considered 'old' (greater than 1 second).
        tokio::time::sleep(Duration::from_secs(1)).await;
        // Sign in again. Now the background task should trigger a republish.
        client.signin(&keypair).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let record3 = client
            .pkarr()
            .resolve_most_recent(&keypair.public_key())
            .await
            .unwrap();
        let ts3 = record3.timestamp().as_u64();

        // Now the republished record's timestamp should be greater than before.
        assert!(
            ts3 > ts2,
            "Record was not republished after threshold exceeded"
        );
    }

    #[tokio::test]
    async fn test_republish_homeserver() {
        // Setup the testnet and run a homeserver.
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();
        // Create a client that will republish conditionally if a record is older than 1 second
        let client = testnet
            .client_builder()
            .max_record_age_micros(1_000_000)
            .build()
            .unwrap();
        let keypair = Keypair::random();

        // Signup publishes a new record.
        client.signup(&keypair, &server.public_key()).await.unwrap();
        // Resolve the record and get its timestamp.
        let record1 = client
            .pkarr()
            .resolve_most_recent(&keypair.public_key())
            .await
            .unwrap();
        let ts1 = record1.timestamp().as_u64();

        // Immediately call republish_homeserver.
        // Since the record is fresh, republish should do nothing.
        client
            .republish_homeserver(&keypair, &server.public_key().to_string())
            .await
            .unwrap();
        let record2 = client
            .pkarr()
            .resolve_most_recent(&keypair.public_key())
            .await
            .unwrap();
        let ts2 = record2.timestamp().as_u64();
        assert_eq!(
            ts1, ts2,
            "Record republished too early; timestamp should be equal"
        );

        // Wait long enough for the record to be considered 'old'.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        // Call republish_homeserver again; now the record should be updated.
        client
            .republish_homeserver(&keypair, &server.public_key().to_string())
            .await
            .unwrap();
        let record3 = client
            .pkarr()
            .resolve_most_recent(&keypair.public_key())
            .await
            .unwrap();
        let ts3 = record3.timestamp().as_u64();
        assert!(
            ts3 > ts2,
            "Record was not republished after threshold exceeded"
        );
    }
}
