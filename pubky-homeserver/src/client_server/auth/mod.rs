//! Self-contained authentication module.
//!
//! Organized into two sub-modules by auth method:
//! - **cookie**: Deprecated cookie-based authentication (session persistence, routes, auth logic)
//! - **jwt**: Grant-based JWT authentication (crypto, persistence, service, routes, auth logic)
//!
//! Shared types:
//! - **session**: [`AuthSession`] enum bridging both auth methods
//! - **middleware**: Authentication layer (Bearer/Cookie), authorization (WriteAccess)
//! - **router**: Pre-configured axum routers for base and tenant routes
//! - **state**: Auth-specific sub-state extracted via `FromRef`

pub mod cookie;
pub mod jwt;
pub mod middleware;
mod router;
mod session;
mod state;

// Re-export crypto submodules at their original paths so external consumers
// (e.g. http_error.rs From impls) don't need path changes.
pub use jwt::crypto::access_jwt_issuer;
pub use jwt::crypto::grant_verifier;
pub use jwt::crypto::pop_verifier;

// Re-export key middleware types for external consumers.
pub use middleware::authentication::AuthenticationLayer;
pub use middleware::authorization::WriteAccess;

pub use router::{base_router, tenant_router};
pub use jwt::service::AuthService;
pub use session::AuthSession;
pub use state::AuthState;
