//! Grant-based authentication: verification, minting, and proof-of-possession.
//!
//! This module contains homeserver-only crypto operations for the grant auth flow:
//! - Grant signature verification
//! - PoP proof verification
//! - Access JWT minting and verification
//! - JWS key format conversion helpers

pub mod access_jwt_issuer;
pub mod grant_verifier;
pub mod jws_crypto;
pub mod pop_verifier;
