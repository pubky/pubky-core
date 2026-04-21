//! Session credentials — the port + two adapters.
//!
//! - [`credential`] holds the [`SessionCredential`] trait (the port).
//! - [`jwt`] is the default adapter: grant + access JWT.
//! - [`cookie`] is the legacy adapter. Retire by deleting the folder.

pub(crate) mod credential;

pub(crate) mod cookie;
pub(crate) mod jwt;

pub(crate) use credential::{SessionCredential, credential_session_missing};
pub(crate) use cookie::credential::CookieCredential;
pub(crate) use jwt::credential::JwtCredential;
