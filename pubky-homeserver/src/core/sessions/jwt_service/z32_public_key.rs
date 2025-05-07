use std::{ops::Deref, str::FromStr};

use pkarr::PublicKey;
use serde::{Deserialize, Serialize};

/// A wrapper around a PublicKey that serializes and deserializes to a z32 string instead of a list of bytes.
#[derive(Debug)]
pub(crate) struct Z32PublicKey(pub PublicKey);

impl Serialize for Z32PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Z32PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self(
            PublicKey::from_str(&s).map_err(serde::de::Error::custom)?,
        ))
    }
}

impl Deref for Z32PublicKey {
    type Target = PublicKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Z32PublicKey {
    /// Pkarr Public Key
    pub fn public_key(&self) -> &PublicKey {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let pubkey =
            PublicKey::from_str("nt4mmqnepy9ipbez3sfsrtjkfpsmf6yuqaumqu8tiejgjgywa5uo").unwrap();
        let z32_pubkey = Z32PublicKey(pubkey);
        let serialized = serde_json::to_string(&z32_pubkey).unwrap();
        let deserialized: Z32PublicKey = serde_json::from_str(&serialized).unwrap();
        assert_eq!(z32_pubkey.0, deserialized.0);
    }
}
