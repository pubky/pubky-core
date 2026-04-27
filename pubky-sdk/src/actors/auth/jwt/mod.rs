//! JWT (grant + `PoP`) authentication flow.
//!
//! The signer returns a user-signed `pubky-grant` JWS which the SDK exchanges
//! for a self-refreshing JWT-backed session. Preferred for long-lived,
//! mirror-friendly sessions.

pub(crate) mod approval;
pub(crate) mod builder;
pub(crate) mod credential;
pub(crate) mod flow;
pub(crate) mod grant_exchange;
pub mod view;

pub use credential::JwtCredential;
pub use flow::PubkyJwtAuthFlow;
pub use view::JwtSessionView;
