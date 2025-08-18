use std::collections::HashMap;
use std::sync::Arc;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{IntoUrl, Method, Response, StatusCode, header::COOKIE};
use url::Url;

use pkarr::{Keypair, PublicKey};
use pubky_common::{
    auth::AuthToken,
    capabilities::{Capabilities, Capability},
    crypto::{decrypt, encrypt, hash, random_bytes},
    session::Session,
};

use crate::{
    BuildError, PubkyClient,
    agent::auth_req::AuthRequest,
    client::pkarr::PublishStrategy,
    errors::{AuthError, Result},
    util::check_http_status,
};

/// Stateful, per-identity API driver that operates atop a shared [PubkyClient].
#[derive(Clone, Debug)]
pub struct PubkyAgent {
    client: Arc<PubkyClient>,
    keypair: Keypair,
    #[cfg(not(target_arch = "wasm32"))]
    sessions: std::sync::Arc<std::sync::RwLock<HashMap<String /* _pubky.<pubky> */, String>>>,
}

impl PubkyAgent {
    pub fn with_client(client: Arc<PubkyClient>, keypair: Keypair) -> Self {
        Self {
            client,
            keypair,
            #[cfg(not(target_arch = "wasm32"))]
            sessions: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Convenience that uses a lazily-initialized default transport.
    pub fn new(keypair: Keypair) -> std::result::Result<Self, BuildError> {
        static DEFAULT: once_cell::sync::OnceCell<Arc<PubkyClient>> =
            once_cell::sync::OnceCell::new();
        let client = DEFAULT.get_or_try_init(|| PubkyClient::new().map(Arc::new))?;
        Ok(Self::with_client(client.clone(), keypair))
    }

    /// Return this agent's public key (by value).
    pub fn pubky(&self) -> PublicKey {
        self.keypair.public_key()
    }

    fn homeserver_url(&self, path: &str) -> String {
        let p = path.trim_start_matches('/');
        format!("pubky://{}/{}", self.pubky(), p)
    }

    async fn request(&self, method: Method, path_or_url: &str) -> Result<reqwest::RequestBuilder> {
        let url = if path_or_url.starts_with("pubky://") || path_or_url.starts_with("http") {
            path_or_url.to_string()
        } else {
            self.homeserver_url(path_or_url)
        };

        let rb = self.client.cross_request(method, &url).await?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let host = format!("_pubky.{}", self.pubky());
            if let Some(secret) = self.sessions.read().unwrap().get(&host).cloned() {
                let cookie_name = self.pubky().to_string();
                return Ok(rb.header(COOKIE, format!("{cookie_name}={secret}")));
            }
        }

        Ok(rb)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn capture_session_cookie(&self, response: &Response) {
        use reqwest::header::SET_COOKIE;
        let cookie_name = self.pubky().to_string();
        let host_key = format!("_pubky.{}", self.pubky());

        for (name, val) in response.headers().iter() {
            if name == SET_COOKIE {
                if let Ok(v) = std::str::from_utf8(val.as_bytes()) {
                    if let Ok(parsed) = cookie::Cookie::parse(v.to_owned()) {
                        if parsed.name() == cookie_name {
                            self.sessions
                                .write()
                                .unwrap()
                                .insert(host_key.clone(), parsed.value().to_string());
                        }
                    }
                }
            }
        }
    }

    // === Generic homeserver verbs (relative to this agent's pubky) ===
    pub async fn get(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn put<B: Into<reqwest::Body>>(&self, path: &str, body: B) -> Result<Response> {
        let resp = self
            .request(Method::PUT, path)
            .await?
            .body(body)
            .send()
            .await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn post<B: Into<reqwest::Body>>(&self, path: &str, body: B) -> Result<Response> {
        let resp = self
            .request(Method::POST, path)
            .await?
            .body(body)
            .send()
            .await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn patch<B: Into<reqwest::Body>>(&self, path: &str, body: B) -> Result<Response> {
        let resp = self
            .request(Method::PATCH, path)
            .await?
            .body(body)
            .send()
            .await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn delete(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn head(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    // === Homeserver identity/session flows ===

    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr public key domain string (`"pubky.<pk>"` variant accepted by server).
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<Session> {
        // 1) Construct the base URL: "https://<homeserver>/signup"
        let mut url = Url::parse(&format!("https://{}", homeserver))?;
        url.set_path("/signup");

        // 2) Optional signup token.
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }

        // 3) Create an AuthToken (root capability).
        let auth_token = AuthToken::sign(&self.keypair, vec![Capability::root()]);
        let request_body = auth_token.serialize();

        // 4) Send POST request with the AuthToken in the body
        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(request_body)
            .send()
            .await?;

        let response = check_http_status(response).await?;

        // 5) Publish the homeserver record
        self.client
            .publish_homeserver(
                &self.keypair,
                Some(&homeserver.to_string()),
                PublishStrategy::Force,
            )
            .await?;

        // 6) Capture session cookie (native)
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&response);

        // 7) Parse the response body into a `Session`
        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }

    /// Check the current session for this Pubky in its homeserver.
    ///
    /// Returns None if not signed in, or [reqwest::Error] if other `>=404`.
    pub async fn session(&self) -> Result<Option<Session>> {
        let response = self
            .request(Method::GET, &format!("pubky://{}/session", self.pubky()))
            .await?
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Signout from the homeserver.
    pub async fn signout(&self) -> Result<()> {
        let response = self
            .request(Method::DELETE, &format!("pubky://{}/session", self.pubky()))
            .await?
            .send()
            .await?;

        check_http_status(response).await?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sessions
                .write()
                .unwrap()
                .remove(&format!("_pubky.{}", self.pubky()));
        }

        Ok(())
    }

    /// Signin to a homeserver.
    /// After a successful signin, republishes the user's PKarr record in background.
    pub async fn signin(&self) -> Result<Session> {
        self.signin_and_ensure_record_published(false).await
    }

    /// Signin and ensure the user's PKarr record is published, optionally syncing.
    pub async fn signin_and_publish(&self) -> Result<Session> {
        self.signin_and_ensure_record_published(true).await
    }

    async fn signin_and_ensure_record_published(&self, publish_sync: bool) -> Result<Session> {
        let token = AuthToken::sign(&self.keypair, vec![Capability::root()]);
        let session = self.signin_with_authtoken(&token).await?;

        if publish_sync {
            self.client
                .publish_homeserver(&self.keypair, None, PublishStrategy::IfOlderThan)
                .await?;
        } else {
            let client_clone = self.client.clone();
            let keypair_clone = self.keypair.clone();
            let future = async move {
                let _ = client_clone
                    .publish_homeserver(&keypair_clone, None, PublishStrategy::IfOlderThan)
                    .await;
            };
            #[cfg(not(target_arch = "wasm32"))]
            tokio::spawn(future);
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(future);
        }

        Ok(session)
    }

    pub async fn send_auth_token<T: IntoUrl>(&self, pubkyauth_url: &T) -> Result<()> {
        let pubkyauth_url = Url::parse(pubkyauth_url.as_str())?;
        let query_params: std::collections::HashMap<String, String> =
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
            .map_err(|e| AuthError::Validation(format!("Invalid base64 secret: {}", e)))?;

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

        let token = AuthToken::sign(&self.keypair, capabilities);
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

        if !token.capabilities().is_empty() {
            self.signin_with_authtoken(&token).await?;
        }

        Ok(token.pubky().clone())
    }

    async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
        let response = self
            .client
            .cross_request(Method::POST, format!("pubky://{}/session", token.pubky()))
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;
        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie(&response);

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }
}
