use once_cell::sync::OnceCell;

use std::sync::Arc;
use url::Url;

use pkarr::{Keypair, PublicKey};

use crate::{
    BuildError, PubkyClient,
    errors::{AuthError, Error, Result},
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

    /// Construct an agent with a fresh random keypair using the shared default transport.
    /// Useful for quick tests in non-persistant flows.
    pub fn new_random() -> std::result::Result<Self, BuildError> {
        Self::new(Some(Keypair::random()))
    }

    /// Returns the known public key, if any.
    pub fn pubky(&self) -> Option<PublicKey> {
        match self.pubky.read() {
            Ok(g) => g.clone(),
            Err(_) => None,
        }
    }

    /// Require a public key; error if unknown.
    pub(crate) fn require_pubky(&self) -> Result<PublicKey> {
        self.pubky()
            .ok_or_else(|| Error::from(AuthError::Validation("Agent has no known pubky".into())))
    }

    /// Require a keypair; error if missing.
    pub(crate) fn require_keypair(&self) -> Result<&Keypair> {
        self.keypair
            .as_ref()
            .ok_or_else(|| Error::from(AuthError::Validation("Agent has no keypair".into())))
    }

    /// Base URL of this agent’s homeserver: `pubky://<pubky>/`.
    pub fn homeserver_base(&self) -> Result<Url> {
        let pk = self.require_pubky()?;
        Url::parse(&format!("pubky://{}/", pk)).map_err(Into::into)
    }
}
