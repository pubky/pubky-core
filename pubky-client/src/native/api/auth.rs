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
use tokio::time::{sleep, Duration};

use super::super::{internal::pkarr::PublishStrategy, Client};
use crate::handle_http_error;

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

        // 6) Publish the homeserver record, retrying a few times if it fails
        {
            let mut last_err = None;
            let max_attempts = 3;
            for attempt in 1..=max_attempts {
                match self
                    .publish_homeserver(
                        keypair,
                        Some(&homeserver.to_string()),
                        PublishStrategy::Force,
                    )
                    .await
                {
                    Ok(_) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        if attempt < max_attempts {
                            // back off before retrying
                            sleep(Duration::from_secs(10)).await;
                        }
                    }
                }
            }

            if let Some(e) = last_err {
                // all attempts failed
                return Err(e);
            }
        }

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
    /// After a successful signin, a background task is spawned to republish the user's
    /// PKarr record if it is missing or older than 1 hour. We don't mind if it succeed
    /// or fails. We want signin to return fast.
    pub async fn signin(&self, keypair: &Keypair) -> Result<Session> {
        self.signin_and_ensure_record_published(keypair, false)
            .await
    }

    /// Signin to a homeserver and ensure the user's PKarr record is published.
    ///
    /// Same as `signin(keypair)` but gives the option to wait for the pkarr packet to be
    /// published in sync. `signin(keypair)` does publish the packet async.
    pub async fn signin_and_ensure_record_published(
        &self,
        keypair: &Keypair,
        publish_sync: bool,
    ) -> Result<Session> {
        let token = AuthToken::sign(keypair, vec![Capability::root()]);
        let session = self.signin_with_authtoken(&token).await?;

        if publish_sync {
            // Wait for the publish to complete.
            self.publish_homeserver(keypair, None, PublishStrategy::IfOlderThan)
                .await?;
        } else {
            // Spawn a background task to republish the record.
            let client_clone = self.clone();
            let keypair_clone = keypair.clone();

            let future = async move {
                // Resolve the record and republish if existing and older MAX_HOMESERVER_RECORD_AGE_SECS
                let _ = client_clone
                    .publish_homeserver(&keypair_clone, None, PublishStrategy::IfOlderThan)
                    .await;
            };
            // Spawn a background task to republish the record.
            #[cfg(not(wasm_browser))]
            tokio::spawn(future);
            #[cfg(wasm_browser)]
            wasm_bindgen_futures::spawn_local(future);
        }

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
    /// no record can be resolved or if the existing record is older than 6 hours.
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
    pub async fn republish_homeserver(&self, keypair: &Keypair, host: &PublicKey) -> Result<()> {
        self.publish_homeserver(
            keypair,
            Some(&host.to_string()),
            PublishStrategy::IfOlderThan,
        )
        .await
    }

    /// Get the homeserver for a given Pubky public key.
    /// Looks up the pkarr packet for the given public key and returns the content of the first `_pubky` SVCB record.
    pub async fn get_homeserver(&self, pubky: &PublicKey) -> Option<String> {
        let packet = self.pkarr.resolve_most_recent(pubky).await?;
        Self::extract_host_from_packet(&packet)
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

    use crate::{native::internal::pkarr::PublishStrategy, Client};

    #[tokio::test]
    async fn test_get_homeserver() {
        let dht = mainline::Testnet::new(3).unwrap();
        let client = Client::builder()
            .pkarr(|builder| builder.bootstrap(&dht.bootstrap))
            .build()
            .unwrap();
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
