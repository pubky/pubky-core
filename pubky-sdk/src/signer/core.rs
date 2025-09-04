use crate::{BuildError, Keypair, PubkyHttpClient, PublicKey, global::global_client};

/// Key holder and signer.
#[derive(Debug, Clone)]
pub struct PubkySigner {
    pub(crate) client: PubkyHttpClient,
    pub(crate) keypair: Keypair,
}

impl PubkySigner {
    /// Construct a Signer atop a specific transport [PubkyHttpClient].
    ///
    /// Choose this when you already manage a long-lived `PubkyHttpClient` (connection reuse, pkarr cache).
    ///
    /// # Examples
    /// ```no_run
    /// # use std::sync::Arc;
    /// # use pubky::{PubkyHttpClient, PubkySigner, Keypair};
    /// let client = Arc::new(PubkyHttpClient::new()?);
    /// let user = PubkySigner::with_client(client.clone(), Keypair::random());
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn with_client(client: &PubkyHttpClient, keypair: Keypair) -> Self {
        Self {
            client: client.clone(),
            keypair,
        }
    }

    /// Construct a Signer using a lazily-initialized, process-wide shared [PubkyHttpClient].
    ///
    /// Choose this when:
    /// - You don’t need to control client construction or lifecycle.
    /// - You want the simplest setup to build your app.
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::{PubkySigner, Keypair};
    /// // Keyless agent: wait for pubkyauth to establish a session
    /// let app  = PubkySigner::new(Keypair::random())?;
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn new(keypair: Keypair) -> std::result::Result<Self, BuildError> {
        let client = global_client()?;
        Ok(Self::with_client(&client, keypair))
    }

    /// Construct a Signer with a fresh random keypair, using the process-wide shared [PubkyHttpClient].
    ///
    /// Purpose:
    /// - Fast ephemeral identities for e2e tests or demos.
    /// - Local experiments where keys are not persisted.
    ///
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::PubkySigner;
    /// let agent = PubkySigner::random()?;
    /// // e.g., `agent.signup(&homeserver_pk, None).await?;`
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn random() -> std::result::Result<Self, BuildError> {
        Self::new(Keypair::random())
    }

    /// Public key of this signer.
    #[inline]
    pub fn public_key(&self) -> PublicKey {
        self.keypair.public_key()
    }

    /// Borrow the agent’s keypair.
    #[inline]
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }
}
