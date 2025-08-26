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
// │  └─ http_targets/            — Platform-specific transport glue (kept minimal).
// │     ├─ mod.rs            — Platform feature gating for native/wasm internals.
// │     ├─ native.rs    — Native `cross_request` delegating to `request`; `prepare_request` no-op.
// │     └─ wasm.rs      — WASM `cross_request`/`prepare_request`: `pubky://` rewrite, endpoint resolution, testnet host/port mapping.
// │
// └─ agent/                  — Stateful identity (“driver”): per-user keys/sessions atop shared PubkyClient.
//    ├─ mod.rs               — Submodule wiring; re-exports public agent API; no logic.
//    ├─ state.rs             — Type-state markers (Keyed/Keyless), sealed trait, and MaybeKeypair wrapper.
//    ├─ core.rs              — `PubkyAgent<S>` (Keyed/Keyless): constructors (new/random/with_client/into_keyless),
//    │                         identity storage (pubky + native session cookie), helpers (`pubky()`).
//    ├─ homeserver.rs        — Namespaced view `Homeserver<'a, S>`: agent-scoped HTTP verbs (GET/PUT/POST/PATCH/DELETE/HEAD)
//    │                         and the `List` Homeserver API methods.
//    ├─ session.rs           — Signup/signin/signout/session flows; cookie capture (native); ensures pkarr republish via `pkdns()`.
//    ├─ auth.rs              — Pubkyauth handshake: `AuthRequest`, `auth_request(..)`, `send_auth_token(..)`,
//    │                         and relay subscription loop.
//    └─ pkdns.rs             — Namespaced PKARR helper view `Pkdns<'a, S>`:
//                              `republish_homeserver_force(..)`, `republish_homeserver_if_stale(..)`, `get_homeserver(..)`,
//                              pulling `max_record_age` & pkarr client from `PubkyClient` and (when needed) using the agent’s keypair.

mod agent;
mod client;
pub mod errors;
mod macros;
mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
pub use agent::state::{KeyedAgent, KeylessAgent};
pub use client::core::{PubkyClient, PubkyClientBuilder};
pub use errors::{BuildError, Error, Result};
// Export common types and constants
pub use crate::agent::path::{FilePath, PubkyPath};
pub use agent::auth::AuthRequest;
pub use agent::homeserver::ListBuilder;
pub use client::core::DEFAULT_RELAYS;
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
