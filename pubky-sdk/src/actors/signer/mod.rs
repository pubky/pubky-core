//! High-level Pubky **signer** actor: sign tokens, `signup`/`signin`, publish PKARR records, and turn it into a session.

pub mod auth;
pub mod core;
pub mod session;

pub use core::PubkySigner;
