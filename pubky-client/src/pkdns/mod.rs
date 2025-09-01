//! PKDNS (Pkarr) top-level actor: resolve & publish `_pubky` records.
//!
//! - **Read-only (no keys):** `PkDns::new()` / `PkDns::with_client(..)`
//! - **Publish (with keys):** `PkDns::with_client_and_keypair(..)` or `signer.pkdns()`
//!
//! Reads do not require a session or keys. Publishing requires a `Keypair`.

pub mod core;
