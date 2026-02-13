// Actual testnet exposed in the library
mod ephemeral_testnet;
mod static_testnet;
mod testnet;

#[cfg(feature = "embedded-postgres")]
mod embedded_postgres;
pub use ephemeral_testnet::{EphemeralTestnet, EphemeralTestnetBuilder};
pub use static_testnet::StaticTestnet;
pub use testnet::Testnet;

// Re-export the core crates
pub use pubky;
pub use pubky_common;
pub use pubky_homeserver;
pub use pubky_test_utils::{drop_test_databases, test};
