use pubky_common::crypto::Keypair;

/// The deterministic keypair used by all testnets (static, ephemeral, and base).
/// Produces pubkey `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`.
pub(crate) fn testnet_keypair() -> Keypair {
    Keypair::from_secret(&[0; 32])
}
