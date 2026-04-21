//! Cookie credential — the legacy session credential.
//!
//! Retirement is a folder delete: `rm -rf credentials/cookie/`, drop the
//! cookie arm in [`crate::actors::session::bootstrap`], and remove the
//! cookie re-export in [`crate::actors::session::mod@crate::actors::session`].

pub(crate) mod credential;
mod legacy_api;
pub(crate) mod secret;
pub mod view;

pub use view::CookieSessionView;
