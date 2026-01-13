use core::fmt;
use core::ops::{Deref, DerefMut};
use core::str::FromStr;
#[cfg(not(target_arch = "wasm32"))]
use std::{io, path::Path};

use serde::{Deserialize, Serialize};

type ParseError = <pkarr::PublicKey as TryFrom<String>>::Error;

fn parse_public_key(value: &str) -> Result<pkarr::PublicKey, ParseError> {
    let raw = if PublicKey::is_pubky_prefixed(value) {
        value.strip_prefix("pubky").unwrap_or(value)
    } else {
        value
    };
    pkarr::PublicKey::try_from(raw.to_string())
}

/// Wrapper around [`pkarr::Keypair`] that customizes [`PublicKey`] rendering.
#[derive(Clone)]
pub struct Keypair(pkarr::Keypair);

impl Keypair {
    /// Generate a random keypair.
    #[must_use]
    pub fn random() -> Self {
        Self(pkarr::Keypair::random())
    }

    /// Export the secret bytes used to derive this keypair.
    #[must_use]
    pub fn secret(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(self.0.secret_key().as_ref());
        out
    }

    /// Construct a [`Keypair`] from a 32-byte secret.
    #[must_use]
    pub fn from_secret(secret: &[u8; 32]) -> Self {
        Self(pkarr::Keypair::from_secret_key(secret))
    }

    /// Read a keypair from a pkarr secret key file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_secret_key_file(path: &Path) -> Result<Self, io::Error> {
        pkarr::Keypair::from_secret_key_file(path).map(Self)
    }

    /// Return the [`PublicKey`] associated with this [`Keypair`].
    ///
    /// Display the returned key with `.to_string()` to get the `pubky<z32>` identifier or
    /// [`PublicKey::z32()`] when you specifically need the bare z-base32 text (e.g. hostnames).
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }

    /// Borrow the inner [`pkarr::Keypair`].
    #[must_use]
    pub const fn as_inner(&self) -> &pkarr::Keypair {
        &self.0
    }

    /// Persist the secret key to disk using the pkarr format.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_secret_key_file(&self, path: &Path) -> Result<(), io::Error> {
        self.0.write_secret_key_file(path)
    }

    /// Extract the inner [`pkarr::Keypair`].
    #[must_use]
    pub fn into_inner(self) -> pkarr::Keypair {
        self.0
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for Keypair {
    type Target = pkarr::Keypair;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Keypair {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<pkarr::Keypair> for Keypair {
    fn from(keypair: pkarr::Keypair) -> Self {
        Self(keypair)
    }
}

impl From<Keypair> for pkarr::Keypair {
    fn from(value: Keypair) -> Self {
        value.0
    }
}

/// Wrapper around [`pkarr::PublicKey`] that renders with the `pubky` prefix.
///
/// Note: serde/transport/database formats continue to use raw z32 strings. Use
/// [`PublicKey::z32()`] for hostnames, query parameters, storage, and wire formats.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicKey(pkarr::PublicKey);

impl PublicKey {
    /// Returns true if the value is in `pubky<z32>` form.
    pub fn is_pubky_prefixed(value: &str) -> bool {
        matches!(value.strip_prefix("pubky"), Some(stripped) if stripped.len() == 52)
    }

    /// Borrow the inner [`pkarr::PublicKey`].
    #[must_use]
    pub const fn as_inner(&self) -> &pkarr::PublicKey {
        &self.0
    }

    /// Extract the inner [`pkarr::PublicKey`].
    #[must_use]
    pub fn into_inner(self) -> pkarr::PublicKey {
        self.0
    }

    /// Return the raw z-base32 representation without the `pubky` prefix.
    ///
    /// This is the canonical transport/storage form used for hostnames, query
    /// parameters, serde, and database persistence.
    #[must_use]
    pub fn z32(&self) -> String {
        self.0.to_string()
    }

    /// Parse a public key from raw z-base32 text (without the `pubky` prefix).
    pub fn try_from_z32(value: &str) -> Result<Self, ParseError> {
        pkarr::PublicKey::try_from(value.to_string()).map(Self)
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pubky{}", self.z32())
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PublicKey").field(&self.to_string()).finish()
    }
}

impl Deref for PublicKey {
    type Target = pkarr::PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<pkarr::PublicKey> for PublicKey {
    fn from(value: pkarr::PublicKey) -> Self {
        Self(value)
    }
}

impl From<&pkarr::PublicKey> for PublicKey {
    fn from(value: &pkarr::PublicKey) -> Self {
        Self(value.clone())
    }
}

impl From<PublicKey> for pkarr::PublicKey {
    fn from(value: PublicKey) -> Self {
        value.0
    }
}

impl From<&PublicKey> for pkarr::PublicKey {
    fn from(value: &PublicKey) -> Self {
        value.0.clone()
    }
}

impl TryFrom<&str> for PublicKey {
    type Error = ParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        parse_public_key(value).map(Self)
    }
}

impl TryFrom<&String> for PublicKey {
    type Error = ParseError;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        parse_public_key(value).map(Self)
    }
}

impl TryFrom<String> for PublicKey {
    type Error = ParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        parse_public_key(&value).map(Self)
    }
}

impl FromStr for PublicKey {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_public_key(s).map(Self)
    }
}
