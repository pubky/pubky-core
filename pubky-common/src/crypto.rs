use rand::prelude::Rng;

pub use pkarr::{Keypair, PublicKey};

pub use ed25519_dalek::Signature;

pub type Hash = blake3::Hash;

pub use blake3::hash;

pub fn random_hash() -> Hash {
    let mut rng = rand::thread_rng();
    Hash::from_bytes(rng.gen())
}
