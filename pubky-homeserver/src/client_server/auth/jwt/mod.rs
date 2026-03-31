//! Grant-based JWT authentication.
//!
//! Contains the crypto primitives, grant/session persistence, auth service,
//! and route handlers for the JWT Bearer token authentication flow.

pub mod auth;
pub mod crypto;
pub mod persistence;
pub mod routes;
pub mod service;
