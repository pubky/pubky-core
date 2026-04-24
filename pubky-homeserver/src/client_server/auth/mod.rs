//! Self-contained authentication module.
//!
//! Organized into two sub-modules by auth method:
//! - **cookie**: Deprecated cookie-based authentication (session persistence, routes, auth logic)
//! - **jwt**: Grant-based JWT authentication (crypto, persistence, service, routes, auth logic)
//!
//! Shared types:
//! - **session**: [`AuthSession`] enum bridging both auth methods
//! - **middleware**: Authentication layer (Bearer/Cookie) and `AuthSession` extractor
//! - **authorization**: [`has_write_permission`] predicate for write handlers
//! - **router**: Pre-configured axum routers for base and tenant routes
//! - **state**: Auth-specific sub-state extracted via `FromRef`

pub mod authorization;
pub mod cookie;
pub mod jwt;
pub mod middleware;
mod router;
mod session;
mod state;

pub use authorization::has_write_permission;
pub use middleware::authentication::AuthenticationLayer;

pub use jwt::service::AuthService;
pub use router::{base_router, tenant_router};
pub use session::AuthSession;
pub use state::AuthState;
