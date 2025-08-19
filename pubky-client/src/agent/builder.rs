use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::RwLock;

use pkarr::Keypair;

use crate::{BuildError, PubkyAgent, PubkyClient};

/// Fluent constructor for `PubkyAgent`.
#[derive(Default)]
pub struct PubkyAgentBuilder {
    client: Option<Arc<PubkyClient>>,
    keypair: Option<Keypair>,
}

impl PubkyAgentBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Provide a shared transport. If omitted, a singleton default client is used.
    pub fn client(mut self, client: Arc<PubkyClient>) -> Self {
        self.client = Some(client);
        self
    }

    /// Attach an in-memory keypair (hot-key mode). Optional.
    pub fn keypair(mut self, keypair: Keypair) -> Self {
        self.keypair = Some(keypair);
        self
    }

    pub fn build(self) -> core::result::Result<PubkyAgent, BuildError> {
        let client = match self.client {
            Some(c) => c,
            None => {
                static DEFAULT: once_cell::sync::OnceCell<Arc<PubkyClient>> =
                    once_cell::sync::OnceCell::new();
                DEFAULT
                    .get_or_try_init(|| PubkyClient::new().map(Arc::new))?
                    .clone()
            }
        };

        Ok(PubkyAgent {
            client,
            keypair: self.keypair,
            pubky: Arc::new(RwLock::new(None)),
            #[cfg(not(target_arch = "wasm32"))]
            session_secret: Arc::new(std::sync::RwLock::new(None)),
        })
    }
}
