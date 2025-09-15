//! Common imports for quick starts.

// Common
pub use crate::{BuildError, Error, Keypair, PublicKey};

// Transport
pub use crate::{PubkyHttpClient, PubkyHttpClientBuilder};

// High level Actors
// Agent to use on behalf of a user on apps.
pub use crate::PubkyAgent;
// Signer to use on behalf of a user on a keychain application.
pub use crate::PubkySigner;
// Authentication flow for apps.
pub use crate::PubkyPairingAuth;
// Homeserver storage API (http verbs + list)
pub use crate::PubkyStorage;
// Pkdns/Pkarr retrieval and publishing
pub use crate::Pkdns;

// Helpers
// Homeserver Paths / URLs
pub use crate::{IntoPubkyResource, PubkyResource};
// Capabilities for auth flows
pub use crate::{Capabilities, Capability};
// Secret recovery utilities
pub use crate::recovery_file;
