//! #![doc = include_str!("../README.md")]
//!

// TODO: deny missing docs.
// #![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
// TODO: deny unwrap only in test
#![cfg_attr(any(), deny(clippy::unwrap_used))]

// Pubky crate codebase map. One stop place to get familiar with this codebase.
//
// src/
// ├─ lib.rs                  — Crate root: declares modules, re-exports public API (PubkyClient, PubkyAgent, errors, types, back-compat aliases).
// ├─ prelude.rs              — Concentrated re-exports for quick-start imports.
// ├─ macros.rs               — Cross-platform logging macro(s) and tiny utilities.
// ├─ errors.rs               — Unified error types (build/request/pkarr/auth) and Result alias.
// ├─ util.rs                 — Library internal utility functions.
// │
// ├─ client/                 — Stateless transport (“engine”): pkarr+DHT resolution and HTTP plumbing.
// │  ├─ mod.rs               — Submodule wiring; `pub use` of PubkyClient and PubkyClientBuilder.
// │  ├─ core.rs              — PubkyClient and builder: reqwest clients, pkarr client, defaults, timeouts, UA, testnet toggles.
// │  ├─ http.rs              — HTTP verb helpers resolving `pubky://` and pkarr-TLD HTTPS into concrete requests.
// │  ├─ list.rs              — Homeserver listing API (`PubkyClient::list()` and `ListBuilder` options + send).
// │  ├─ pkarr.rs             — Record publish/extract helpers and `PublishStrategy` (no HTTP changes).
// │  └─ internal/            — Platform-specific transport glue (kept minimal).
// │     ├─ mod.rs            — Platform feature gating for native/wasm internals.
// │     ├─ cookies.rs        — Native cookie jar for ICANN domains only; ignores `_pubky.<pubkey>` to prevent session leakage.
// │     ├─ http_native.rs    — Native `cross_request` delegating to `request`; `prepare_request` no-op.
// │     └─ http_wasm.rs      — WASM `cross_request`/`prepare_request`: `pubky://` rewrite, endpoint resolution, testnet host/port mapping.
// │
// └─ agent/                  — Stateful identity (“driver”): per-user keys and sessions atop shared transport PubkyClient.
//    ├─ mod.rs               — Submodule wiring; `pub use` of PubkyAgent; no logic.
//    ├─ core.rs              — PubkyAgent struct: holds keypair, Arc<PubkyClient>, per-agent session cache, request builder.
//    ├─ verbs.rs             — Agent-scoped HTTP verbs targeting the agent’s homeserver (GET/PUT/POST/PATCH/DELETE/HEAD).
//    ├─ session.rs           — Signup/signin/signout/session flows; ensures pkarr republish on signin (sync/async).
//    └─ auth_req.rs          — Auth handshake types and logic (`AuthRequest`, `auth_request`, relay subscription).

mod agent;
mod client;
pub mod errors;
mod macros;
mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
pub use agent::{core::PubkyAgent, state::KeyedAgent, state::KeylessAgent};
pub use client::core::{PubkyClient, PubkyClientBuilder};
pub use errors::{BuildError, Error, Result};
// Export common types.
pub use agent::auth::AuthRequest;
pub use client::list::ListBuilder;
// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    capabilities::{Capabilities, Capability},
    recovery_file,
    session::Session,
};

// --- Back-compat aliases (soft-deprecated) ---
#[allow(deprecated)]
pub type Client = PubkyClient;
#[allow(deprecated)]
pub type ClientBuilder = PubkyClientBuilder;
