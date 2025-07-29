use anyhow::{Result, anyhow};
use base64::{Engine, alphabet::URL_SAFE, engine::general_purpose::NO_PAD};
use reqwest::{Method, Url};
use std::collections::HashMap;
use std::future::Future;

use pkarr::{Keypair, PublicKey};
use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    crypto::{decrypt, encrypt, hash, random_bytes},
    session::Session,
};

use crate::{Client, http_client::HttpClient, internal::pkarr::PublishStrategy};

impl<H: HttpClient> Client<H> {
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
        // 1. Construct the signup URL.
        let mut url = Url::parse(&format!("https://{}/signup", homeserver))?;
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }

        // 2. Create and serialize the authentication token.
        let auth_token = AuthToken::sign(keypair, vec![Capability::root()]);
        let request_body = auth_token.serialize();

        // 3. Perform the request using the abstract HttpClient.
        let response_bytes = self
            .http
            .request(Method::POST, url, Some(request_body), None)
            .await?;

        // 4. Publish the homeserver record. This now happens before deserializing the session.
        self.publish_homeserver(
            keypair,
            Some(&homeserver.to_string()),
            PublishStrategy::Force,
        )
        .await?;

        // 5. Deserialize the session from the response bytes.
        Ok(Session::deserialize(&response_bytes)?)
    }

    /// Check the current session for a given Pubky in its homeserver.
    ///
    /// Returns None  if not signed in, or [reqwest::Error]
    /// if the response has any other `>=404` status code.
    pub async fn session(&self, pubky: &PublicKey) -> Result<Option<Session>> {
        let url = Url::parse(&format!("pubky://{}/session", pubky))?;

        match self.http.request(Method::GET, url, None, None).await {
            Ok(bytes) => Ok(Some(Session::deserialize(&bytes)?)),
            Err(e) => {
                // Check for a 404 Not Found error to return Ok(None).
                // This is a pragmatic way to handle it with a generic error type.
                if e.to_string().contains("404") {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }
    /// Signout from a homeserver.
    pub async fn signout(&self, pubky: &PublicKey) -> Result<()> {
        let url = Url::parse(&format!("pubky://{}/session", pubky))?;
        self.http.request(Method::DELETE, url, None, None).await?;
        Ok(())
    }

    /// Signin to a homeserver.
    /// After a successful signin, a background task is spawned to republish the user's
    /// PKarr record if it is missing or older than 1 hour. We don't mind if it succeed
    /// or fails. We want signin to return fast.
    pub async fn signin(&self, keypair: &Keypair) -> Result<Session> {
        let token = AuthToken::sign(keypair, vec![Capability::root()]);
        let session = self.signin_with_authtoken(&token).await?;

        // The responsibility of running this in the background is moved to the caller.
        // The core library now performs the action synchronously for simplicity.
        self.publish_homeserver(keypair, None, PublishStrategy::IfOlderThan)
            .await?;

        Ok(session)
    }

    /// Send an authorization token to a relay for a pubkyauth request.
    pub async fn send_auth_token(&self, keypair: &Keypair, pubkyauth_url_str: &str) -> Result<()> {
        let pubkyauth_url = Url::parse(&pubkyauth_url_str.replace("pubkyauth_url", "http"))?;
        let query_params: HashMap<String, String> =
            pubkyauth_url.query_pairs().into_owned().collect();

        let relay = query_params
            .get("relay")
            .and_then(|r| Url::parse(r).ok())
            .ok_or_else(|| anyhow!("Missing or invalid 'relay' in pubkyauth URL"))?;
        let client_secret: [u8; 32] = query_params
            .get("secret")
            .and_then(|s| {
                base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD)
                    .decode(s)
                    .ok()
            })
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| anyhow!("Missing or invalid 'secret' in pubkyauth URL"))?;

        let capabilities = query_params
            .get("caps")
            .map(|s| {
                s.split(',')
                    .filter_map(|sub| Capability::try_from(sub).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let token = AuthToken::sign(keypair, capabilities);
        let encrypted_token = encrypt(&token.serialize(), &client_secret);

        let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);
        let mut callback_url = relay;
        let channel_id = engine.encode(hash(&client_secret).as_bytes());
        callback_url
            .path_segments_mut()
            .map_err(|_| anyhow!("Cannot modify relay URL path"))?
            .pop_if_empty()
            .push(&channel_id);

        self.http
            .request(Method::POST, callback_url, Some(encrypted_token), None)
            .await?;
        Ok(())
    }

    /// Internal helper to sign in using a pre-made `AuthToken`.
    pub(crate) async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
        let url = Url::parse(&format!("pubky://{}/session", token.pubky()))?;
        let response_bytes = self
            .http
            .request(Method::POST, url, Some(token.serialize()), None)
            .await?;
        Ok(Session::deserialize(&response_bytes)?)
    }

    /// Return `pubkyauth://` url and wait for the incoming [AuthToken]
    /// verifying that AuthToken, and if capabilities were requested, signing in to
    /// the Pubky's homeserver and returning the [Session] information.
    pub fn auth_request(
        &self,
        relay_url_str: &str,
        capabilities: &Capabilities,
    ) -> Result<AuthRequest<H>> {
        let mut relay = Url::parse(relay_url_str)?;
        let (url, client_secret) = Self::create_auth_request_url(&mut relay, capabilities)?;

        Ok(AuthRequest {
            url,
            relay,
            client_secret,
            client: self.clone(),
        })
    }

    /// Internal helper to construct the `pubkyauth://` URL.
    fn create_auth_request_url(
        relay: &mut Url,
        capabilities: &Capabilities,
    ) -> Result<(Url, [u8; 32])> {
        let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);
        let client_secret: [u8; 32] = random_bytes::<32>();
        let secret_encoded = engine.encode(client_secret);

        let pubkyauth_url = Url::parse(&format!(
            "pubkyauth:///?caps={}&secret={}&relay={}",
            capabilities, secret_encoded, relay
        ))?;

        let channel_id = engine.encode(hash(&client_secret).as_bytes());
        relay
            .path_segments_mut()
            .map_err(|_| anyhow!("Cannot modify relay URL path"))?
            .pop_if_empty()
            .push(&channel_id);

        Ok((pubkyauth_url, client_secret))
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

/// Represents a pending authentication request.
/// This struct is now generic and holds a clone of the client.
#[derive(Debug, Clone)]
pub struct AuthRequest<H: HttpClient> {
    url: Url,
    relay: Url,
    client_secret: [u8; 32],
    client: Client<H>,
}

impl<H: HttpClient> AuthRequest<H> {
    /// Returns the `pubkyauth://` URL that should be presented to the user.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Waits for the user to respond to the auth request.
    /// This method now contains the long-polling logic and must be awaited.
    pub fn response(&self) -> impl Future<Output = Result<PublicKey>> + '_ {
        async move {
            // This loop performs long-polling against the relay server.
            let encrypted_token = loop {
                match self
                    .client
                    .http
                    .request(Method::GET, self.relay.clone(), None, None)
                    .await
                {
                    Ok(bytes) => break bytes,
                    Err(e) => {
                        // A simple timeout check. In a real scenario, more robust
                        // error handling (e.g., exponential backoff) might be needed.
                        if e.to_string().contains("timeout") {
                            continue;
                        }
                        return Err(e);
                    }
                }
            };

            let token_bytes = decrypt(&encrypted_token, &self.client_secret)
                .map_err(|e| anyhow!("Got invalid token: {}", e))?;
            let token = AuthToken::verify(&token_bytes)?;

            if !token.capabilities().is_empty() {
                self.client.signin_with_authtoken(&token).await?;
            }

            Ok(token.pubky().clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{NativeClient, internal::pkarr::PublishStrategy};
    use pkarr::Keypair;

    #[tokio::test]
    async fn test_get_homeserver() {
        let dht = mainline::Testnet::new(3).unwrap();
        let mut config = NativeClient::config();
        config.pkarr(|builder| builder.bootstrap(&dht.bootstrap));

        let client = NativeClient::from_config(config).unwrap();
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
