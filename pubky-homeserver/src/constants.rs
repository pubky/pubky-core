#[cfg(test)]
use pkarr::Keypair;


/// The default keypair used for testing.
/// May also be used as uncritical default value.
#[cfg(test)]
pub fn default_keypair() -> Keypair {
    let secret_hex: &str = "0000000000000000000000000000000000000000000000000000000000000000"; // HEX
    let secret: [u8; 32] = hex::decode(secret_hex).expect("is always valid hex").try_into().expect("is always 32 bytes");
    Keypair::from_secret_key(&secret)
}

// The default limit of a list api if no `limit` query parameter is provided.
pub const DEFAULT_LIST_LIMIT: u16 = 100;
pub const DEFAULT_MAX_LIST_LIMIT: u16 = 1000;