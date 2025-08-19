use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use once_cell::sync::OnceCell;
use reqwest::{IntoUrl, Method, Response, StatusCode, header::COOKIE};
use std::collections::HashMap;
use std::sync::Arc;
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
    errors::{AuthError, Error, Result},
    util::check_http_status,
};

/// Stateful, per-identity API driver that operates atop a shared [PubkyClient].
#[derive(Clone, Debug)]
pub struct PubkyAgent {
    pub(crate) client: Arc<PubkyClient>,

    /// Optional identity material. Supports keyless agents.
    pub(crate) keypair: Option<Keypair>,

    /// Known public key for this agent (derived from keypair or pubkyauth).
    pub(crate) pubky: Arc<std::sync::RwLock<Option<PublicKey>>>,

    /// Per-agent session cookie secret for `_pubky.<pubky>` (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) session_secret: Arc<std::sync::RwLock<Option<String>>>,
}

impl PubkyAgent {
    pub fn with_client(client: Arc<PubkyClient>, keypair: Option<Keypair>) -> Self {
        let initial_pubky = keypair.as_ref().map(|k| k.public_key());
        Self {
            client,
            keypair,
            pubky: Arc::new(std::sync::RwLock::new(initial_pubky)),
            #[cfg(not(target_arch = "wasm32"))]
            session_secret: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Convenience that uses a lazily-initialized default transport.
    pub fn new(keypair: Option<Keypair>) -> std::result::Result<Self, BuildError> {
        static DEFAULT: OnceCell<Arc<PubkyClient>> = OnceCell::new();
        let client = DEFAULT.get_or_try_init(|| PubkyClient::new().map(Arc::new))?;
        Ok(Self::with_client(client.clone(), keypair))
    }

    /// Returns the known public key, if any.
    pub fn pubky(&self) -> Option<PublicKey> {
        match self.pubky.read() {
            Ok(g) => g.clone(),
            Err(_) => None,
        }
    }

    /// Require a public key; error if unknown.
    fn require_pubky(&self) -> Result<PublicKey> {
        self.pubky()
            .ok_or_else(|| Error::from(AuthError::Validation("Agent has no known pubky".into())))
    }

    /// Require a keypair; error if missing.
    fn require_keypair(&self) -> Result<&Keypair> {
        self.keypair
            .as_ref()
            .ok_or_else(|| Error::from(AuthError::Validation("Agent has no keypair".into())))
    }

    /// Base URL of this agent’s homeserver: `pubky://<pubky>/`.
    fn homeserver_base(&self) -> Result<Url> {
        let pk = self.require_pubky()?;
        Url::parse(&format!("pubky://{}/", pk)).map_err(Into::into)
    }

    /// Build a request. If `path_or_url` is relative, targets this agent’s homeserver.
    async fn request(&self, method: Method, path_or_url: &str) -> Result<reqwest::RequestBuilder> {
        let url = match Url::parse(path_or_url) {
            Ok(abs) => abs,
            Err(_) => {
                let mut base = self.homeserver_base()?;
                base.set_path(path_or_url);
                base
            }
        };

        let rb = self.client.cross_request(method, url.clone()).await?;

        // Attach session cookie only when the target host is this agent’s homeserver.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let matches_agent = self
                .pubky()
                .and_then(|pk| {
                    let host = url.host_str().unwrap_or("");
                    if host.starts_with("_pubky.") {
                        let tail = &host["_pubky.".len()..];
                        PublicKey::try_from(tail).ok().map(|h| h == pk)
                    } else {
                        PublicKey::try_from(host).ok().map(|h| h == pk)
                    }
                })
                .unwrap_or(false);

            if matches_agent {
                if let Ok(g) = self.session_secret.read() {
                    if let Some(secret) = g.as_ref() {
                        let cookie_name = self.require_pubky()?.to_string();
                        return Ok(rb.header(COOKIE, format!("{cookie_name}={secret}")));
                    }
                }
            }
        }

        Ok(rb)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn capture_session_cookie_for(&self, response: &Response, pubky: &PublicKey) {
        use reqwest::header::SET_COOKIE;
        let cookie_name = pubky.to_string();

        for (name, val) in response.headers().iter() {
            if name == SET_COOKIE {
                if let Ok(v) = std::str::from_utf8(val.as_bytes()) {
                    if let Ok(parsed) = cookie::Cookie::parse(v.to_owned()) {
                        if parsed.name() == cookie_name {
                            if let Ok(mut slot) = self.session_secret.write() {
                                *slot = Some(parsed.value().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn capture_session_cookie(&self, response: &Response) -> Result<()> {
        let pk = self.require_pubky()?;
        self.capture_session_cookie_for(response, &pk);
        Ok(())
    }

    // === Homeserver verbs relative to this agent’s pubky ===

    pub async fn get(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::GET, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
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
        let _ = self.capture_session_cookie(&resp);
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
        let _ = self.capture_session_cookie(&resp);
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
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn delete(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::DELETE, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    pub async fn head(&self, path: &str) -> Result<Response> {
        let resp = self.request(Method::HEAD, path).await?.send().await?;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.capture_session_cookie(&resp);
        check_http_status(resp).await
    }

    // === Session/identity flows ===

    /// Signup to a homeserver and publish `_pubky` record. Requires a keypair.
    pub async fn signup(
        &self,
        homeserver: &PublicKey,
        signup_token: Option<&str>,
    ) -> Result<Session> {
        let kp = self.require_keypair()?;

        let mut url = Url::parse(&format!("https://{}", homeserver))?;
        url.set_path("/signup");
        if let Some(token) = signup_token {
            url.query_pairs_mut().append_pair("signup_token", token);
        }

        let auth_token = AuthToken::sign(kp, vec![Capability::root()]);
        let response = self
            .client
            .cross_request(Method::POST, url)
            .await?
            .body(auth_token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;

        self.client
            .publish_homeserver(kp, Some(&homeserver.to_string()), PublishStrategy::Force)
            .await?;

        // On successful signup, the agent’s pubky is known from the keypair.
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(kp.public_key());
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.capture_session_cookie_for(&response, &kp.public_key());

        let bytes = response.bytes().await?;
        Ok(Session::deserialize(&bytes)?)
    }

    /// Retrieve session for current pubky. Fails if pubky is unknown.
    pub async fn session(&self) -> Result<Option<Session>> {
        let response = self.request(Method::GET, "/session").await?.send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Signout from homeserver and clear this agent’s cookie.
    pub async fn signout(&self) -> Result<()> {
        let response = self
            .request(Method::DELETE, "/session")
            .await?
            .send()
            .await?;
        check_http_status(response).await?;

        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut slot) = self.session_secret.write() {
            *slot = None;
        }
        Ok(())
    }

    /// Signin by locally signing an AuthToken. Requires keypair.
    pub async fn signin(&self) -> Result<Session> {
        self.signin_and_ensure_record_published(false).await
    }

    /// Signin and publish `_pubky` if stale. Requires keypair.
    pub async fn signin_and_publish(&self) -> Result<Session> {
        self.signin_and_ensure_record_published(true).await
    }

    async fn signin_and_ensure_record_published(&self, publish_sync: bool) -> Result<Session> {
        let kp = self.require_keypair()?;

        // Ensure agent knows its pubky
        if let Ok(mut g) = self.pubky.write() {
            *g = Some(kp.public_key());
        }

        let token = AuthToken::sign(kp, vec![Capability::root()]);
        let session = self.signin_with_authtoken(&token).await?;

        if publish_sync {
            self.client
                .publish_homeserver(kp, None, PublishStrategy::IfOlderThan)
                .await?;
        } else {
            let client = self.client.clone();
            let kp_cloned = kp.clone();
            let fut = async move {
                let _ = client
                    .publish_homeserver(&kp_cloned, None, PublishStrategy::IfOlderThan)
                    .await;
            };
            #[cfg(not(target_arch = "wasm32"))]
            tokio::spawn(fut);
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(fut);
        }

        Ok(session)
    }

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

    async fn signin_with_authtoken(&self, token: &AuthToken) -> Result<Session> {
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
