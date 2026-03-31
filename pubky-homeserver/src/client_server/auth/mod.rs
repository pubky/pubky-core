//! Self-contained authentication module for the grant-based JWT auth flow.
//!
//! Owns the full auth vertical slice:
//! - **crypto**: Grant verification, PoP verification, Access JWT minting
//! - **persistence**: Grant, GrantSession, PopNonce repositories
//! - **middleware**: Authentication layer (Bearer/Cookie), authorization (WriteAccess)
//! - **routes**: Signin, grant session creation, session management
//! - **service**: AuthService facade orchestrating the full auth flow
//! - **router**: Pre-configured axum routers for base and tenant routes

pub mod crypto;
pub mod middleware;
pub mod persistence;
mod router;
pub mod routes;
mod service;

// Re-export crypto submodules at their original paths so external consumers
// (e.g. http_error.rs From impls) don't need path changes.
pub use crypto::access_jwt_issuer;
pub use crypto::grant_verifier;
pub use crypto::pop_verifier;

// Re-export key middleware types for external consumers.
pub use middleware::authentication::AuthenticationLayer;
pub use middleware::authorization::WriteAccess;

pub use router::{base_router, tenant_router};
pub use service::AuthService;
