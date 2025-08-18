//! Common imports for quick starts.

// Common
pub use crate::{BuildError, Error, Keypair, PublicKey};

// Transport
pub use crate::{PubkyClient, PubkyClientBuilder};

// Agent to use on behalf of a user
pub use crate::PubkyAgent;

// Auth and listing helpers:
pub use crate::{api::auth::AuthRequest, api::public::ListBuilder};

// Capabilities for auth flows
pub use crate::{Capabilities, Capability};

// Recovery utilities
pub use crate::recovery_file;
