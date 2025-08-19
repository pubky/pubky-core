use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{IntoUrl, Method};
use url::Url;

use pkarr::PublicKey;
use pubky_common::{
    auth::AuthToken,
    crypto::{decrypt, encrypt, hash, random_bytes},
};

use crate::errors::{AuthError, Result};
use crate::util::check_http_status;

use crate::{Capabilities, Capability, PubkyAgent, Session};

#[derive(Debug, Clone)]
pub struct AuthRequest {
    url: Url,
    rx: flume::Receiver<Result<PublicKey>>,
}

impl AuthRequest {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub async fn response(&self) -> Result<PublicKey> {
        match self.rx.recv_async().await {
            Ok(result_from_task) => result_from_task,
            Err(_) => Err(AuthError::RequestExpired.into()),
        }
    }
}

impl PubkyAgent {
    /// Send a signed AuthToken to a relay channel. Requires keypair.
    pub async fn send_auth_token<T: IntoUrl>(&self, pubkyauth_url: &T) -> Result<()> {
        let kp = self.require_keypair()?;

        let pubkyauth_url = Url::parse(pubkyauth_url.as_str())?;
        let query_params: HashMap<String, String> =
            pubkyauth_url.query_pairs().into_owned().collect();

        let relay_str = query_params
            .get("relay")
            .ok_or_else(|| AuthError::Validation("Missing 'relay' query parameter".to_string()))?;
        let relay = Url::parse(relay_str)?;

        let secret_str = query_params
            .get("secret")
            .ok_or_else(|| AuthError::Validation("Missing 'secret' query parameter".to_string()))?;

        let secret_bytes = URL_SAFE_NO_PAD
            .decode(secret_str)
            .map_err(|e| AuthError::Validation(format!("Invalid base64 secret: {e}")))?;

        let client_secret: [u8; 32] = secret_bytes
            .try_into()
            .map_err(|_| AuthError::Validation("Client secret must be 32 bytes".to_string()))?;

        let capabilities = query_params
            .get("caps")
            .map(|caps_string| {
                caps_string
                    .split(',')
                    .filter_map(|cap| Capability::try_from(cap).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let token = AuthToken::sign(kp, capabilities);
        let encrypted_token = encrypt(&token.serialize(), &client_secret);

        let mut callback_url = relay.clone();
        let mut path_segments = callback_url
            .path_segments_mut()
            .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        path_segments.pop_if_empty();
        let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
        path_segments.push(&channel_id);
        drop(path_segments);

        let response = self
            .client
            .cross_request(Method::POST, callback_url)
            .await?
            .body(encrypted_token)
            .send()
            .await?;

        check_http_status(response).await?;
        Ok(())
    }

    /// Create an auth request and spawn a listener for the response token.
    pub fn auth_request<T: IntoUrl>(
        &self,
        relay: T,
        capabilities: &Capabilities,
    ) -> Result<AuthRequest> {
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

        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(future);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(future);

        Ok(AuthRequest { url, rx })
    }

    fn create_auth_request(
        &self,
        relay: &mut Url,
        capabilities: &Capabilities,
    ) -> Result<(Url, [u8; 32])> {
        let client_secret: [u8; 32] = random_bytes::<32>();

        let pubkyauth_url = Url::parse(&format!(
            "pubkyauth:///?caps={capabilities}&secret={}&relay={relay}",
            URL_SAFE_NO_PAD.encode(client_secret)
        ))?;

        let mut segments = relay
            .path_segments_mut()
            .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        segments.pop_if_empty();
        let channel_id = &URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
        segments.push(channel_id);
        drop(segments);

        Ok((pubkyauth_url, client_secret))
    }

    async fn subscribe_to_auth_response(
        &self,
        relay: Url,
        client_secret: &[u8; 32],
        tx: flume::Sender<Result<PublicKey>>,
    ) -> Result<PublicKey> {
        let response = loop {
            match self
                .client
                .cross_request(Method::GET, relay.clone())
                .await?
                .send()
                .await
            {
                Ok(response) => break Ok(response),
                Err(error) => {
                    if error.is_timeout() && !tx.is_disconnected() {
                        crate::cross_debug!("Connection to HttpRelay timed out, reconnecting...");
                        continue;
                    }
                    break Err(error);
                }
            }
        }?;

        let encrypted_token = response.bytes().await?;
        let token_bytes = decrypt(&encrypted_token, client_secret)?;
        let token = AuthToken::verify(&token_bytes)?;

        // Update known pubky from the token.
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(token.pubky().clone());
        }

        // If capabilities were requested, sign in to establish session cookies.
        if !token.capabilities().is_empty() {
            self.signin_with_authtoken(&token).await?;
        }

        Ok(token.pubky().clone())
    }

    pub(crate) async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
        let url = format!("pubky://{}/session", token.pubky());
        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;

        // Remember pubky and capture cookie for this identity.
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(token.pubky().clone());
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie_for(&response, token.pubky());

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }
}
