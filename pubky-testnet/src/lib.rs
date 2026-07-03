// Actual testnet exposed in the library
mod common;
mod ephemeral_testnet;
mod static_testnet;
mod testnet;

#[cfg(feature = "docker-postgres")]
pub mod docker_postgres;

/// Deprecated module alias. Use [`docker_postgres`] instead.
#[cfg(feature = "docker-postgres")]
#[deprecated(since = "0.9.0", note = "Renamed to `docker_postgres`")]
pub mod embedded_postgres {
    pub use crate::docker_postgres::*;
}

pub use ephemeral_testnet::{EphemeralTestnet, EphemeralTestnetBuilder};
pub use static_testnet::{StaticTestnet, StaticTestnetBuilder};
pub use testnet::Testnet;

// Re-export the core crates
pub use pubky;
pub use pubky_common;
pub use pubky_homeserver;
pub use pubky_test_utils::{drop_test_databases, test};
