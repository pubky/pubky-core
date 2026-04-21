//! JWT credential — the default session credential.
//!
//! Contains the full JWT flow in one folder: [`credential`] holds the
//! `JwtCredential` implementation, [`view`] exposes JWT-only operations to
//! callers of [`PubkySession`](crate::actors::session::core::PubkySession),
//! and [`grant_exchange`] holds the factory functions that turn a
//! user-signed grant into a ready `Arc<dyn SessionCredential>`.

pub(crate) mod credential;
pub(crate) mod grant_exchange;
pub mod view;

pub use view::JwtSessionView;
