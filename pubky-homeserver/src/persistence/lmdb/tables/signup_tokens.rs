use super::super::LmDB;
use base32::{encode, Alphabet};
use heed::{
    types::{Bytes, Str},
    Database,
};
use pkarr::PublicKey;
use postcard::{from_bytes, to_allocvec};
use pubky_common::{crypto::random_bytes, timestamp::Timestamp};
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
    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("serialize signup token")
    }

    pub fn deserialize(bytes: &[u8]) -> Self {
        from_bytes(bytes).expect("deserialize signup token")
    }

    pub fn is_used(&self) -> bool {
        self.used.is_some()
    }

    // Generate 7 random bytes and encode as BASE32, fully uppercase
    // with hyphens every 4 characters. Example, `QXV0-15V7-EXY0`
    pub fn random() -> Self {
        let bytes = random_bytes::<7>();
        let encoded = encode(Alphabet::Crockford, &bytes).to_uppercase();
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

impl LmDB {
    pub fn generate_signup_token(&mut self) -> anyhow::Result<String> {
        let signup_token = SignupToken::random();
        let mut wtxn = self.env.write_txn()?;
        self.tables
            .signup_tokens
            .put(&mut wtxn, &signup_token.token, &signup_token.serialize())?;
        wtxn.commit()?;
        Ok(signup_token.token)
    }

    pub fn validate_and_consume_signup_token(
        &self,
        token: &str,
        user_pubkey: &PublicKey,
    ) -> anyhow::Result<()> {
        let mut wtxn = self.env.write_txn()?;
        if let Some(token_bytes) = self.tables.signup_tokens.get(&wtxn, token)? {
            let mut signup_token = SignupToken::deserialize(token_bytes);
            if signup_token.is_used() {
                anyhow::bail!("Token already used");
            }
            // Mark token as used.
            signup_token.used = Some(user_pubkey.clone());
            self.tables
                .signup_tokens
                .put(&mut wtxn, token, &signup_token.serialize())?;
            wtxn.commit()?;
            Ok(())
        } else {
            anyhow::bail!("Invalid token");
        }
    }
}

pub type SignupTokensTable = Database<Str, Bytes>;
