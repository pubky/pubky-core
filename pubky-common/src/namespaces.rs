//! Namespaces using to prepend signed messages to avoid collisions.

/// Pubky Auth namespace as defined at the [spec](https://pubky.github.io/pubky-core/spec/auth.html)
pub const PUBKY_AUTH: &[u8; 10] = b"PUBKY:AUTH";
