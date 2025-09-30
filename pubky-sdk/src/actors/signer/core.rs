use crate::{BuildError, Keypair, PubkyHttpClient, PublicKey};

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
            client: PubkyHttpClient::new()?,
            keypair,
        })
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
