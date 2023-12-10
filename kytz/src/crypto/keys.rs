//! Keypairs.

use crate::crypto::Key;

/// Generate a random secret seed.
pub fn generate_seed() -> Key {
    rand::random()
}
