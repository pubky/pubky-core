//! Client <=> Signer authing (“pubkyauth”) as a single, self-contained flow.
//!
//! ## TL;DR (happy path)
//!
//! ### Sign in (legacy / cookie)
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow, AuthFlowKind};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let flow = PubkyAuthFlow::start(&caps, AuthFlowKind::signin())?; // starts background polling immediately
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! // Blocks until the signer (e.g., Pubky Ring) approves and server issues a session.
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ### Sign up (legacy / cookie)
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow, AuthFlowKind, PublicKey};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let homeserver_public_key: PublicKey = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo".parse().unwrap();
//! let signup_token = "1234567890";
//! let flow = PubkyAuthFlow::start(&caps, AuthFlowKind::signup(homeserver_public_key, Some(signup_token.to_string())))?; // starts background polling immediately
//! println!("Scan to sign up: {}", flow.authorization_url());
//!
//! // Blocks until the signer (e.g., Pubky Ring) approves and server issues a session.
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ### Sign in (JWT — grant + `PoP`)
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow, AuthFlowKind, ClientId};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyAuthFlow::start_jwt(&caps, AuthFlowKind::signin(), client_id)?;
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! // Resolves to a JWT-backed session that self-refreshes.
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ## Custom relay / non-blocking UI
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow, AuthFlowKind};
//! # use std::time::Duration;
//! # async fn ui() -> pubky::Result<()> {
//! let flow = PubkyAuthFlow::builder(&Capabilities::default(), AuthFlowKind::signin())
//!     .relay(url::Url::parse("http://localhost:8080/inbox/")?) // your relay
//!     .start()?; // starts background polling immediately
//!
//! // show_qr(flow.authorization_url()); // render QR or deeplink in your UI
//!
//! loop {
//!     if let Some(session) = flow.try_poll_once().await? {
//!         // on_logged_in(session);
//!         break;
//!     }
//!     tokio::time::sleep(Duration::from_millis(300)).await;
//! }
//! # Ok(()) }
//! ```
//!
//! ## How it works (security)
//! Each flow generates a random `client_secret` (32 bytes). The relay **channel id**
//! is `base64url( hash(client_secret) )`. The signer encrypts an `AuthToken` with
//! `client_secret` and POSTs it to the channel; your app long-polls `GET` on the same
//! URL and decrypts the payload locally. The relay **cannot decrypt anything**, it
//! simply forwards bytes.

use url::Url;

use pubky_common::{
    auth::jws::ClientId,
    crypto::{Keypair, random_bytes},
};

use crate::{
    AuthToken, Capabilities, PubkyHttpClient, PubkySession, PublicKey,
    actors::{
        DEFAULT_HTTP_RELAY_INBOX,
        auth::{
            approval::{AuthApproval, AuthApprovalMode},
            deep_links::{
                DeepLink, SigninDeepLink, SigninJwtDeepLink, SignupDeepLink, SignupJwtDeepLink,
            },
            relay::auth_relay_listener::AuthRelayListener,
        },
        session::bootstrap::{SessionBootstrapContext, session_from_auth_approval},
    },
    errors::{AuthError, Result},
};

/// End-to-end **auth flow** (request + live polling) you *hold on to*.
///
/// Supports both sign in and sign up flows.
///
/// Use it like this:
/// 1. Construct with one of:
///    - [`PubkyAuthFlow::start`] / [`PubkyAuthFlow::builder`] for the legacy (cookie) flow.
///    - [`PubkyAuthFlow::start_jwt`] / [`PubkyAuthFlow::jwt_builder`] for the grant + `PoP` (JWT) flow.
/// 2. Display [`authorization_url`](Self::authorization_url) (QR/deeplink) to the signer.
/// 3. Complete the flow with [`await_approval`](Self::await_approval) **or**
///    poll with [`try_poll_once`](Self::try_poll_once) / [`try_token`](Self::try_token).
///
/// Background polling **starts immediately** at construction. Dropping this value cancels
/// the background task; the relay channel itself expires server-side after its TTL.
#[derive(Debug)]
pub struct PubkyAuthFlow {
    relay_listener: AuthRelayListener,
    client: PubkyHttpClient,
    approval_mode: AuthApprovalMode,
    session_bootstrap_ctx: Option<SessionBootstrapContext>,
    auth_url: DeepLink,
}

impl PubkyAuthFlow {
    /// Start a legacy (cookie) flow with the default HTTP relay.
    ///
    /// Spawns the background poller immediately and returns a handle.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing [`PubkyHttpClient`]
    ///   or generating the relay URL fails.
    pub fn start(caps: &Capabilities, auth_kind: AuthFlowKind) -> Result<Self> {
        LegacyAuthFlowBuilder::new(caps.clone(), auth_kind).start()
    }

    /// Create a builder for the legacy (cookie) flow to override the **relay** and/or
    /// provide a custom **client**.
    #[must_use]
    pub fn builder(caps: &Capabilities, auth_kind: AuthFlowKind) -> LegacyAuthFlowBuilder {
        LegacyAuthFlowBuilder::new(caps.clone(), auth_kind)
    }

    /// Start a JWT (grant + `PoP`) flow with the default HTTP relay.
    ///
    /// The resulting [`PubkySession`] is JWT-backed and self-refreshes.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing [`PubkyHttpClient`]
    ///   or generating the relay URL fails.
    pub fn start_jwt(
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
        client_id: ClientId,
    ) -> Result<Self> {
        JwtAuthFlowBuilder::new(caps.clone(), auth_kind, client_id).start()
    }

    /// Create a builder for the JWT (grant + `PoP`) flow to override the **relay**,
    /// provide a custom **client**, or pin a specific **`PoP` keypair**.
    #[must_use]
    pub fn jwt_builder(
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
        client_id: ClientId,
    ) -> JwtAuthFlowBuilder {
        JwtAuthFlowBuilder::new(caps.clone(), auth_kind, client_id)
    }

    /// The `pubkyauth://` deep link you display (QR/URL) to the signer.
    ///
    /// Contains the **capabilities**, **`client_secret`** (base64url), and **relay** base.
    #[must_use]
    pub fn authorization_url(&self) -> Url {
        self.auth_url.clone().into()
    }

    /// Block until the signer approves and the server issues a session.
    ///
    /// This awaits the background poller’s result, verifies/decrypts the token,
    /// and completes the `/session` exchange to return a ready-to-use [`PubkySession`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures while polling the relay.
    /// - Propagates errors from internal session bootstrap if it fails.
    pub async fn await_approval(self) -> Result<PubkySession> {
        let Self {
            relay_listener,
            client,
            approval_mode,
            session_bootstrap_ctx,
            auth_url: _,
        } = self;
        let approval = Self::await_decoded_approval(relay_listener, approval_mode).await?;

        session_from_auth_approval(client, session_bootstrap_ctx, approval).await
    }

    /// Block until the signer approves and we receive an [`AuthToken`].
    ///
    /// This awaits the background poller’s result.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures encountered while polling the relay.
    pub async fn await_token(self) -> Result<AuthToken> {
        let Self {
            relay_listener,
            client: _,
            approval_mode,
            session_bootstrap_ctx: _,
            auth_url: _,
        } = self;

        Self::legacy_token_from_approval(
            Self::await_decoded_approval(relay_listener, approval_mode).await?,
            "received a grant payload; use await_approval() instead of await_token()",
        )
    }

    /// Non-blocking probe (single step) that **consumes any ready token** and returns:
    /// - `Ok(Some(session))` when a token was delivered and the session established.
    /// - `Ok(None)` if no payload yet (keep polling later).
    /// - `Err(e)` on transport/server errors or if the channel expired.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expired before a token arrived.
    /// - Propagates HTTP/transport failures from constructing the session.
    pub async fn try_poll_once(&self) -> Result<Option<PubkySession>> {
        let Some(approval) = self.try_decoded_approval()? else {
            return Ok(None);
        };

        Ok(Some(
            session_from_auth_approval(
                self.client.clone(),
                self.session_bootstrap_ctx.clone(),
                approval,
            )
            .await?,
        ))
    }

    /// Non-blocking check: returns a verified `AuthToken` if the background poller has delivered it.
    ///
    /// - `Some(Ok(AuthToken))` when ready.
    /// - `Some(Err(_))` if the background task failed (expired/transport error).
    /// - `None` if not yet delivered.
    #[must_use]
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        let approval = match self.try_decoded_approval() {
            Ok(Some(approval)) => approval,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };

        Some(Self::legacy_token_from_approval(
            approval,
            "received a grant payload; use try_poll_once() for grant flows",
        ))
    }

    async fn await_decoded_approval(
        relay_listener: AuthRelayListener,
        approval_mode: AuthApprovalMode,
    ) -> Result<AuthApproval> {
        let message = relay_listener.await_message().await?;
        AuthApproval::decode(&message, approval_mode)
    }

    fn try_decoded_approval(&self) -> Result<Option<AuthApproval>> {
        let Some(message) = self.relay_listener.try_message() else {
            return Ok(None);
        };

        Ok(Some(AuthApproval::decode(&message?, self.approval_mode)?))
    }

    fn legacy_token_from_approval(
        approval: AuthApproval,
        grant_message: &str,
    ) -> Result<AuthToken> {
        match approval {
            AuthApproval::Legacy(token) => Ok(*token),
            AuthApproval::Grant { .. } => Err(AuthError::Validation(grant_message.into()).into()),
        }
    }
}

/// The kind of authentication flow to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFlowKind {
    /// Sign in to an existing account.
    SignIn,
    /// Sign up for a new account.
    SignUp {
        /// The public key of the homeserver to sign up on.
        homeserver_public_key: Box<PublicKey>,
        /// The signup token to use for the signup flow.
        /// This is optional.
        signup_token: Option<String>,
    },
}

impl AuthFlowKind {
    /// Create a sign in flow.
    #[must_use]
    pub fn signin() -> Self {
        Self::SignIn
    }

    /// Create a sign up flow.
    /// # Arguments
    /// * `homeserver_public_key` - The public key of the homeserver to sign up on.
    /// * `signup_token` - The signup token to use for the signup flow. This is optional.
    #[must_use]
    pub fn signup(homeserver_public_key: PublicKey, signup_token: Option<String>) -> Self {
        Self::SignUp {
            homeserver_public_key: Box::new(homeserver_public_key),
            signup_token,
        }
    }
}

/// Builder for the **legacy (cookie)** [`PubkyAuthFlow`].
///
/// The signer returns a [`pubky_common::auth::AuthToken`] which the SDK exchanges for
/// a session cookie. For long-lived, mirror-friendly sessions, prefer
/// [`JwtAuthFlowBuilder`] instead.
#[derive(Debug, Clone)]
pub struct LegacyAuthFlowBuilder {
    caps: Capabilities,
    base_relay: Url,
    client: Option<PubkyHttpClient>,
    auth_kind: AuthFlowKind,
    client_secret: [u8; 32],
}

impl LegacyAuthFlowBuilder {
    pub(crate) fn new(caps: Capabilities, auth_kind: AuthFlowKind) -> Self {
        Self {
            caps,
            base_relay: Url::parse(DEFAULT_HTTP_RELAY_INBOX)
                .expect("Should be able to parse the default HTTP relay"),
            client: None,
            auth_kind,
            client_secret: random_bytes::<32>(),
        }
    }

    /// Set a custom relay base URL. Trailing slash optional.
    #[must_use]
    pub fn relay(mut self, url: Url) -> Self {
        self.base_relay = url;
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    #[must_use]
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Override the random `client_secret`. By default, a fresh 32-byte secret is generated.
    #[must_use]
    pub fn client_secret(mut self, client_secret: [u8; 32]) -> Self {
        self.client_secret = client_secret;
        self
    }

    /// Finalize: derive channel, compute the `pubkyauth://` deep link, spawn the background poller,
    /// and return the flow handle.
    ///
    /// # Errors
    /// - Propagates failures from constructing the default [`PubkyHttpClient`] or starting
    ///   the [`AuthRelayListener`].
    pub fn start(self) -> Result<PubkyAuthFlow> {
        finalize(
            &self.caps,
            &self.auth_kind,
            self.base_relay,
            self.client,
            self.client_secret,
            None,
        )
    }
}

/// Builder for the **JWT (grant + `PoP`)** [`PubkyAuthFlow`].
///
/// When this builder is used:
/// - The deep link gains `cid=<client_id>` and `cpk=<client_pk_z32>` params.
/// - The signer (Ring) signs a `pubky-grant` JWS instead of a legacy
///   [`AuthToken`](pubky_common::auth::AuthToken).
/// - The resulting [`PubkySession`] is JWT-backed and self-refreshes.
///
/// Apps that want a long-lived, mirror-friendly session should use this builder.
#[derive(Debug, Clone)]
pub struct JwtAuthFlowBuilder {
    caps: Capabilities,
    base_relay: Url,
    client: Option<PubkyHttpClient>,
    auth_kind: AuthFlowKind,
    client_secret: [u8; 32],
    client_id: ClientId,
    client_keypair: Option<Keypair>,
}

impl JwtAuthFlowBuilder {
    pub(crate) fn new(caps: Capabilities, auth_kind: AuthFlowKind, client_id: ClientId) -> Self {
        Self {
            caps,
            base_relay: Url::parse(DEFAULT_HTTP_RELAY_INBOX)
                .expect("Should be able to parse the default HTTP relay"),
            client: None,
            auth_kind,
            client_secret: random_bytes::<32>(),
            client_id,
            client_keypair: None,
        }
    }

    /// Set a custom relay base URL. Trailing slash optional.
    #[must_use]
    pub fn relay(mut self, url: Url) -> Self {
        self.base_relay = url;
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    #[must_use]
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Override the random `client_secret`. By default, a fresh 32-byte secret is generated.
    #[must_use]
    pub fn client_secret(mut self, client_secret: [u8; 32]) -> Self {
        self.client_secret = client_secret;
        self
    }

    /// Pin a specific Ed25519 keypair as the grant's `cnf` claim and `PoP` signer.
    /// If omitted, a fresh random keypair is generated at [`Self::start`].
    #[must_use]
    pub fn client_keypair(mut self, keypair: Keypair) -> Self {
        self.client_keypair = Some(keypair);
        self
    }

    /// Finalize: derive channel, compute the `pubkyauth://` deep link, spawn the background poller,
    /// and return the flow handle.
    ///
    /// # Errors
    /// - Propagates failures from constructing the default [`PubkyHttpClient`] or starting
    ///   the [`AuthRelayListener`].
    pub fn start(self) -> Result<PubkyAuthFlow> {
        let keypair = self.client_keypair.unwrap_or_else(Keypair::random);
        finalize(
            &self.caps,
            &self.auth_kind,
            self.base_relay,
            self.client,
            self.client_secret,
            Some((self.client_id, keypair)),
        )
    }
}

/// Shared finalization for both builders. Resolves the default client, assembles the
/// deep link, chooses the approval mode, wires the session bootstrap context, and
/// spawns the relay listener.
fn finalize(
    caps: &Capabilities,
    auth_kind: &AuthFlowKind,
    base_relay: Url,
    client: Option<PubkyHttpClient>,
    client_secret: [u8; 32],
    grant_binding: Option<(ClientId, Keypair)>,
) -> Result<PubkyAuthFlow> {
    let client = match client {
        Some(c) => c,
        None => PubkyHttpClient::new()?,
    };

    let auth_url = create_url(
        caps,
        auth_kind,
        &base_relay,
        client_secret,
        grant_binding
            .as_ref()
            .map(|(cid, kp)| (cid.clone(), kp.public_key())),
    );

    // For the SignUp grant flow, the homeserver public key is taken from
    // the AuthFlowKind. For the SignIn grant flow, the homeserver pubkey is
    // resolved from PKARR after we receive the grant — see AuthRelayListener.
    let signup_homeserver = match auth_kind {
        AuthFlowKind::SignUp {
            homeserver_public_key,
            signup_token,
        } => Some((*homeserver_public_key.clone(), signup_token.clone())),
        AuthFlowKind::SignIn => None,
    };

    let approval_mode = if grant_binding.is_some() {
        AuthApprovalMode::GrantJwt
    } else {
        AuthApprovalMode::LegacyToken
    };

    let session_bootstrap_ctx = grant_binding.map(|(_, client_keypair)| SessionBootstrapContext {
        client_keypair,
        signup_homeserver,
    });

    let relay_listener = AuthRelayListener::builder(client_secret)
        .relay_base_url(base_relay)
        .client(client.clone())
        .start()?;

    Ok(PubkyAuthFlow {
        relay_listener,
        client,
        approval_mode,
        session_bootstrap_ctx,
        auth_url,
    })
}

/// Build the typed deep link for the flow. The variant chosen depends on the
/// [`AuthFlowKind`] (signin vs signup) and whether a grant binding is present
/// (legacy cookie vs JWT grant + `PoP`).
fn create_url(
    caps: &Capabilities,
    auth_kind: &AuthFlowKind,
    base_relay: &Url,
    client_secret: [u8; 32],
    grant_binding: Option<(ClientId, PublicKey)>,
) -> DeepLink {
    match (auth_kind, grant_binding) {
        (AuthFlowKind::SignIn, Some((cid, cpk))) => DeepLink::SigninJwt(SigninJwtDeepLink::new(
            caps.clone(),
            base_relay.clone(),
            client_secret,
            cid,
            cpk,
        )),
        (AuthFlowKind::SignIn, None) => DeepLink::Signin(SigninDeepLink::new(
            caps.clone(),
            base_relay.clone(),
            client_secret,
        )),
        (
            AuthFlowKind::SignUp {
                homeserver_public_key,
                signup_token,
            },
            Some((cid, cpk)),
        ) => DeepLink::SignupJwt(SignupJwtDeepLink::new(
            caps.clone(),
            base_relay.clone(),
            client_secret,
            *homeserver_public_key.clone(),
            signup_token.clone(),
            cid,
            cpk,
        )),
        (
            AuthFlowKind::SignUp {
                homeserver_public_key,
                signup_token,
            },
            None,
        ) => DeepLink::Signup(SignupDeepLink::new(
            caps.clone(),
            base_relay.clone(),
            client_secret,
            *homeserver_public_key.clone(),
            signup_token.clone(),
        )),
    }
}
