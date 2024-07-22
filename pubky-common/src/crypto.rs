use rand::prelude::Rng;

pub use pkarr::{Keypair, PublicKey};

pub use ed25519_dalek::Signature;

pub type Hash = blake3::Hash;

pub use blake3::hash;

pub fn random_hash() -> Hash {
    let mut rng = rand::thread_rng();
    Hash::from_bytes(rng.gen())
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut rng = rand::thread_rng();
    let mut arr = [0u8; N];

    #[allow(clippy::needless_range_loop)]
    for i in 0..N {
        arr[i] = rng.gen();
    }
    arr
}
