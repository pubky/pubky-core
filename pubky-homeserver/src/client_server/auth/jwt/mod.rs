//! Grant-based JWT authentication (current auth flow).
//!
//! This module adds per-app sessions using
//! short-lived JWTs bound to long-lived, revocable **Grants**.
//!
//! # Design principles
//!
//! - **Cold key model**: Ring (the user's key manager) signs only at Grant creation.
//!   The client app refreshes JWTs autonomously using PoP proofs — no Ring needed.
//! - **Mirror-friendly**: a Grant has no `aud` claim. The same Grant can be exchanged
//!   at any homeserver in the user's pkarr record.
//! - **JWT as lightweight pointer**: capabilities and metadata live server-side (keyed
//!   by `jti`), keeping the token small and enabling instant revocation.
//! - **PoP binding**: prevents Grant replay by third parties and homeservers. With
//!   WebCrypto non-extractable keys, also prevents Grant theft via XSS.
//!
//! # Token types
//!
//! | Token | Signer | Lifetime | Purpose |
//! |-------|--------|----------|---------|
//! | **Grant** (`pubky-grant`) | User keypair | Long-lived | Delegates scoped capabilities to a client app |
//! | **PoP proof** (`pubky-pop`) | Client keypair | ±3 min | Proves possession of the key bound by Grant `cnf` |
//! | **Access JWT** (`JWT`) | Homeserver keypair | 1 hour | Bearer token for API requests |
//!
//! All tokens use EdDSA (Ed25519) in JWS Compact Serialization. Each Grant carries a
//! self-declared `client_id` (domain string, like OAuth public clients) for session
//! separation — the security boundary is capability scoping, not `client_id`.
//!
//! # Flow
//!
//! ## 1. Session creation (`POST /session`, JSON body)
//!
//! The client sends a **Grant JWS** + **PoP JWS**. The homeserver verifies both
//! (signature, expiry, PoP audience/nonce/timestamp), stores the Grant idempotently,
//! mints an Access JWT, and inserts a session row (max 1 per Grant; oldest evicted).
//!
//! ## 2. Authenticating requests
//!
//! [`JwtAuthenticationMiddleware`](middleware::JwtAuthenticationMiddleware) runs **before**
//! cookie middleware. It verifies the `Authorization: Bearer` JWT, looks up the session
//! by `jti`, and checks the Grant is not revoked/expired. A present-but-invalid Bearer
//! token is rejected immediately (no cookie fallback).
//!
//! ## 3. Grant management (root capability required)
//!
//! - `GET /sessions` — list active Grants.
//! - `DELETE /session/{grant_id}` — revoke a Grant and delete all its sessions.
//!
//! ## 4. Replay protection
//!
//! - **Nonce**: each PoP nonce is tracked in `pop_nonces` (unique constraint, GC after 360 s).
//! - **Audience**: PoP `aud` must match this homeserver's public key.

pub mod auth;
pub mod crypto;
pub mod middleware;
pub mod persistence;
pub mod routes;
pub mod service;
