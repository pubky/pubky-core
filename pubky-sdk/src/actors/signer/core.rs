use crate::{BuildError, Keypair, PubkyHttpClient, PublicKey, global::global_client};

/// Key holder and signer.
#[derive(Debug, Clone)]
pub struct PubkySigner {
    pub(crate) client: PubkyHttpClient,
    pub(crate) keypair: Keypair,
}

impl PubkySigner {
    /// Construct a new PubkySigner.
    ///
    /// This is your entry point to keychain managing tooling.
    ///
    /// # Examples
    /// ```
    /// # use pubky::{PubkySigner, Keypair};
    /// let keypair = Keypair::random();
    /// let app  = PubkySigner::new(keypair)?;
    /// # Ok::<_, pubky::BuildError>(())
    /// ```
    pub fn new(keypair: Keypair) -> std::result::Result<Self, BuildError> {
        Ok(Self {
            client: global_client()?,
            keypair,
        })
    }

    /// Construct a Signer with a fresh random keypair, using the process-wide shared [PubkyHttpClient].
    ///
    /// Purpose:
    /// - Fast ephemeral identities for e2e tests or demos.
    /// - Local experiments where keys are not persisted.
    ///
    ///
    /// # Examples
    /// ```
    /// # use pubky::PubkySigner;
    /// let signer = PubkySigner::random()?;
    /// // e.g., `signer.signup(&homeserver_pk, None).await?;`
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

    /// Borrow the signer's keypair.
    #[inline]
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }
}
