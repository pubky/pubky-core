use heed::{
    types::{Bytes, Str},
    Database,
};
use pkarr::PublicKey;
use postcard::from_bytes;

use serde::{Deserialize, Serialize};

pub const SIGNUP_TOKENS_TABLE: &str = "signup_tokens";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignupToken {
    pub token: String,
    pub created_at: u64,
    /// If Some(pubkey), the token has been used.
    pub used: Option<PublicKey>,
}

impl SignupToken {
    pub fn deserialize(bytes: &[u8]) -> Self {
        from_bytes(bytes).expect("deserialize signup token")
    }
}

#[cfg(test)]
impl SignupToken {
    pub fn serialize(&self) -> Vec<u8> {
        use postcard::to_allocvec;
        to_allocvec(self).expect("serialize signup token")
    }

    // Generate 7 random bytes and encode as BASE32, fully uppercase
    // with hyphens every 4 characters. Example, `QXV0-15V7-EXY0`
    pub fn random() -> Self {
        use pubky_common::{crypto::random_bytes, timestamp::Timestamp};

        let bytes = random_bytes::<7>();
        let encoded = base32::encode(base32::Alphabet::Crockford, &bytes).to_uppercase();
        let mut with_hyphens = String::new();
        for (i, ch) in encoded.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                with_hyphens.push('-');
            }
            with_hyphens.push(ch);
        }

        SignupToken {
            token: with_hyphens,
            created_at: Timestamp::now().as_u64(),
            used: None,
        }
    }
}

pub type SignupTokensTable = Database<Str, Bytes>;
