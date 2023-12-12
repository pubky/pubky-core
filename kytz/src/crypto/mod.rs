pub mod encryption;
pub mod passphrase;
pub mod seed;

/// A 32 bytes key (encryption key or public key or shared_secret key).
pub type Key = [u8; bessie::KEY_LEN];
/// A 24 bytes Nonce or salt.
pub type Nonce = [u8; bessie::NONCE_LEN];

/// Generate a random secret seed.
pub fn generate_seed() -> Key {
    rand::random()
}

/// Generate a random secret seed.
pub fn generate_salt() -> Nonce {
    rand::random()
}
