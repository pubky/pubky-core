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

use crate::handle_http_error;

use super::super::Client;

impl Client {
    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    ///
    /// - `keypair`: The user's keypair (used to sign the AuthToken).
    /// - `homeserver`: The server's public key (as a domain-like string).
    /// - `signup_token`: Optional invite code or token required by the server for new users.
    pub async fn signup(
        &self,
        keypair: &Keypair,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<Session> {
        // 1) Construct the base URL: "https://<homeserver>/signup"
        let mut url = Url::parse(&format!("https://{}", homeserver))?;
        url.set_path("/signup");

        // 2) If we have a signup_token, append it to the query string.
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }

        // 3) Create an AuthToken (e.g. with root capability).
        let auth_token = AuthToken::sign(keypair, vec![Capability::root()]);
        let request_body = auth_token.serialize();

        // 4) Send POST request with the AuthToken in the body
        let response = self
            .cross_request(Method::POST, url)
            .await
            .body(request_body)
            .send()
            .await?;

        // 5) Check for non-2xx status codes
        handle_http_error!(response);

        // 6) Publish the homeserver record
        self.publish_homeserver(keypair, &homeserver.to_string())
            .await?;

        // 7) Store session cookie in local store
        #[cfg(not(target_arch = "wasm32"))]
        self.cookie_store
            .store_session_after_signup(&response, &keypair.public_key());

        // 8) Parse the response body into a `Session`
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
    pub async fn signin(&self, keypair: &Keypair) -> Result<Session> {
        let token = AuthToken::sign(keypair, vec![Capability::root()]);

        self.signin_with_authtoken(&token).await
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
    use pkarr::Keypair;
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_testnet::Testnet;
    use reqwest::StatusCode;
    use std::time::Duration;

    #[tokio::test]
    async fn basic_authn() {
        let testnet = Testnet::run().await.unwrap();
        let server = testnet.run_homeserver().await.unwrap();

        let client = testnet.client_builder().build().unwrap();

        let keypair = Keypair::random();

        client
            .signup(&keypair, &server.public_key(), None)
            .await
            .unwrap();

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

            client
                .signup(&keypair, &server.public_key(), None)
                .await
                .unwrap();

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
            .signup(&first_keypair, &server.public_key(), None)
            .await
            .unwrap();

        client
            .signup(&second_keypair, &server.public_key(), None)
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
            client
                .signup(&keypair, &server.public_key(), None)
                .await
                .unwrap();

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

    #[tokio::test]
    async fn test_signup_with_token() {
        // 1. Start a test homeserver with closed signups (i.e. signup tokens required)
        let testnet = Testnet::run().await.unwrap();
        let admin_password = "testPassword".to_string();
        let server = unsafe {
            testnet
                .homeserver_builder()
                .close_signups()
                .admin_password(admin_password.clone()) // configure this test homeserver to require signup tokens
                .run()
                .await
                .unwrap()
        };

        let client = testnet.client_builder().build().unwrap();
        let keypair = Keypair::random();

        // 2. Try to signup with an invalid token "AAAAA" and expect failure.
        let invalid_signup = client
            .signup(&keypair, &server.public_key(), Some("AAAAA"))
            .await;
        assert!(
            invalid_signup.is_err(),
            "Signup should fail with an invalid signup token"
        );

        // 3. Call the admin endpoint to generate a valid signup token.
        //    The admin endpoint is protected via the header "X-Admin-Password"
        //    and the password we set up above.
        let admin_url = format!(
            "https://{}/admin/generate_signup_token",
            server.public_key()
        );

        // 3.1. Call the admin endpoint *with a WRONG admin password* to ensure we get 401 UNAUTHORIZED.
        let wrong_password_response = client
            .get(&admin_url)
            .header("X-Admin-Password", "notTheRightPassword")
            .send()
            .await
            .unwrap();
        assert_eq!(
            wrong_password_response.status(),
            StatusCode::UNAUTHORIZED,
            "Wrong admin password should return 401"
        );

        // 3.1 Now call the admin endpoint again, this time with the correct password.
        let admin_response = client
            .get(&admin_url)
            .header("X-Admin-Password", &admin_password)
            .send()
            .await
            .unwrap();
        assert_eq!(
            admin_response.status(),
            StatusCode::OK,
            "Admin endpoint should return OK"
        );
        let valid_token = admin_response.text().await.unwrap(); // The token string.

        println!("VALID TOKEN: {valid_token}");
        // 4. Now signup with the valid token. Expect success and a session back.
        let session = client
            .signup(&keypair, &server.public_key(), Some(&valid_token))
            .await
            .unwrap();
        assert!(
            !session.pubky().to_string().is_empty(),
            "Session should contain a valid public key"
        );

        // 5. Finally, sign in with the same keypair and verify that a session is returned.
        let signin_session = client.signin(&keypair).await.unwrap();
        assert_eq!(
            signin_session.pubky(),
            &keypair.public_key(),
            "Signed-in session should correspond to the same public key"
        );
    }
}
