use once_cell::sync::OnceCell;

use std::sync::Arc;
use url::Url;

use pkarr::{Keypair, PublicKey};

use crate::{
    BuildError, PubkyClient,
    agent::state::{Keyed, Keyless, MaybeKeypair, sealed::Sealed},
    errors::{AuthError, Error, Result},
};

/// Stateful, per-identity API driver built on a shared [`PubkyClient`].
///
/// An `PubkyAgent` represents one user/identity. It optionally holds a `Keypair` (for
/// self-signed flows like `signin()`/`signup()`), and always tracks the user’s `pubky`
/// (either from the keypair or learned later via the pubkyauth flow). On native targets,
/// each agent also owns exactly one session cookie secret; cookies never leak across agents.
///
/// What it does:
/// - Attaches the correct session cookie to requests that target this agent’s homeserver
///   (`pubky://<pubky>/...` or `https://_pubky.<pubky>/...`), and to nothing else.
/// - Exposes homeserver verbs (`get/put/post/patch/delete/head`) scoped to this identity.
/// - Implements identity flows: `signup`, `signin`, `signout`, `session`, and pubkyauth.
///
/// When to use:
/// - Use `PubkyAgent` whenever you’re acting “as a user” against a Pubky homeserver.
/// - Use `PubkyClient` only for raw transport or unauthenticated/public operations.
///
/// Concurrency:
/// - `PubkyAgent` is cheap to clone and thread-safe; it shares the underlying `PubkyClient`.
#[derive(Clone, Debug)]
pub struct PubkyAgent<S: Sealed> {
    pub(crate) client: Arc<PubkyClient>,

    /// Optional identity material. If `None`, this is a keyless agent suited for pubkyauth
    /// flows initiated by third-party apps. Methods that require signing (e.g. `signin`,
    /// `signup`, `send_auth_token`) will error without a keypair.
    pub(crate) keypair: MaybeKeypair<S>,

    /// Known public key for this agent (from the keypair or learned via pubkyauth).
    pub(crate) pubky: Arc<std::sync::RwLock<Option<PublicKey>>>,

    /// Native-only, single session cookie secret for `_pubky.<pubky>`. Never shared across agents.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) session_secret: Arc<std::sync::RwLock<Option<String>>>,
}

impl PubkyAgent<Keyless> {
    /// Keyless agent on a specific transport. Use for third-party apps initiating pubkyauth.
    ///
    /// Choose this when:
    /// - You already manage a long-lived `PubkyClient` (connection reuse, pkarr cache).
    /// - You are spawning multiple agents and want them to share transport resources.
    ///
    /// If a `keypair` is provided, the agent’s `pubky` is initialized from it; otherwise the
    /// `pubky` becomes known later (e.g., after a pubkyauth handshake).
    ///
    /// # Examples
    /// ```no_run
    /// # use std::sync::Arc;
    /// # use pubky::{PubkyClient, PubkyAgent, Keypair};
    /// let client = Arc::new(PubkyClient::new()?);
    /// let user = PubkyAgent::with_client(client.clone(), Some(Keypair::random()));
    /// let app  = PubkyAgent::with_client(client.clone(), None); // keyless
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn with_client(client: Arc<PubkyClient>) -> Self {
        Self {
            client,
            keypair: MaybeKeypair::new_none(),
            pubky: Arc::new(std::sync::RwLock::new(None)),
            #[cfg(not(target_arch = "wasm32"))]
            session_secret: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Construct an agent using a lazily-initialized, process-wide shared `PubkyClient`.
    ///
    /// Choose this when:
    /// - You don’t need to control client construction or lifecycle.
    /// - You want the simplest setup in tests, CLIs, or small apps.
    ///
    /// If a `keypair` is provided, the agent can call `signin()`/`signup()` directly.
    /// If `None`, use pubkyauth (`auth_request` on a third-party, and `send_auth_token`
    /// on the authenticating agent) to establish a session later.
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::{PubkyAgent, Keypair};
    /// // Keyed agent: direct signin
    /// let user = PubkyAgent::new(Some(Keypair::random()))?;
    /// // Keyless agent: wait for pubkyauth to establish a session
    /// let app  = PubkyAgent::new(None)?;
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn new() -> std::result::Result<Self, BuildError> {
        static DEFAULT: OnceCell<Arc<PubkyClient>> = OnceCell::new();
        let client = DEFAULT.get_or_try_init(|| PubkyClient::new().map(Arc::new))?;
        Ok(Self::with_client(client.clone()))
    }
}

impl PubkyAgent<Keyed> {
    /// Construct an agent atop a specific shared transport, optionally with a keypair.
    ///
    /// Choose this when:
    /// - You already manage a long-lived `PubkyClient` (connection reuse, pkarr cache).
    /// - You are spawning multiple agents and want them to share transport resources.
    ///
    /// If a `keypair` is provided, the agent’s `pubky` is initialized from it; otherwise the
    /// `pubky` becomes known later (e.g., after a pubkyauth handshake).
    ///
    /// # Examples
    /// ```no_run
    /// # use std::sync::Arc;
    /// # use pubky::{PubkyClient, PubkyAgent, Keypair};
    /// let client = Arc::new(PubkyClient::new()?);
    /// let user = PubkyAgent::with_client(client.clone(), Keypair::random());
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn with_client(client: Arc<PubkyClient>, keypair: Keypair) -> Self {
        let pubky = keypair.public_key();
        Self {
            client,
            keypair: MaybeKeypair::new(keypair),
            pubky: Arc::new(std::sync::RwLock::new(Some(pubky))),
            #[cfg(not(target_arch = "wasm32"))]
            session_secret: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Construct an agent using a lazily-initialized, process-wide shared `PubkyClient`.
    ///
    /// Choose this when:
    /// - You don’t need to control client construction or lifecycle.
    /// - You want the simplest setup to build your app.
    ///
    /// If a `keypair` is provided, the agent can call `signin()`/`signup()` directly.
    /// If `None`, use pubkyauth (`auth_request` on a third-party, and `send_auth_token`
    /// on the authenticating agent) to establish a session later.
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::{PubkyAgent, Keypair};
    /// // Keyless agent: wait for pubkyauth to establish a session
    /// let app  = PubkyAgent::new()?;
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn new(keypair: Keypair) -> std::result::Result<Self, BuildError> {
        static DEFAULT: OnceCell<Arc<PubkyClient>> = OnceCell::new();
        let client = DEFAULT.get_or_try_init(|| PubkyClient::new().map(Arc::new))?;
        Ok(Self::with_client(client.clone(), keypair))
    }

    /// Construct an agent with a fresh random keypair, using the default shared `PubkyClient`.
    ///
    /// Purpose:
    /// - Fast ephemeral identities for e2e tests or demos.
    /// - Local experiments where keys are not persisted.
    ///
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::PubkyAgent;
    /// let agent = PubkyAgent::random()?;
    /// // e.g., `agent.signup(&homeserver_pk, None).await?;`
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn random() -> std::result::Result<Self, BuildError> {
        Self::new(Keypair::random())
    }

    /// Drop the keypair, producing a keyless agent that keeps the session and pubky.
    pub fn into_keyless(self) -> PubkyAgent<Keyless> {
        PubkyAgent {
            client: self.client,
            keypair: MaybeKeypair::new_none(),
            pubky: self.pubky,
            #[cfg(not(target_arch = "wasm32"))]
            session_secret: self.session_secret,
        }
    }
}

impl<S: Sealed> PubkyAgent<S> {
    /// Returns the known public key, if any.
    /// An Agent will only have a known pubky if 1) it was initalized with a `KeyPair` or 2)
    /// it has signed-in using the auth protocol.
    pub fn pubky(&self) -> Option<PublicKey> {
        match self.pubky.read() {
            Ok(g) => g.clone(),
            Err(_) => None,
        }
    }

    pub(crate) fn set_pubky_if_empty(&self, pk: &PublicKey) {
        if let Ok(mut g) = self.pubky.write() {
            if g.is_none() {
                *g = Some(pk.clone());
            }
        }
    }

    /// Require a public key; error if unknown.
    pub(crate) fn require_pubky(&self) -> Result<PublicKey> {
        self.pubky()
            .ok_or_else(|| Error::from(AuthError::Validation("Agent has no known pubky".into())))
    }
}
