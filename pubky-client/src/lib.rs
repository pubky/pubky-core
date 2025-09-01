//! #![doc = include_str!("../README.md")]
//! **pubky sdk** a small, ergonomic library for building Pubky applications.
//!
//! # Quick start
//! ```no_run
//! use pubky::prelude::*;
//!
//! # async fn run() -> pubky::Result<()> {
//! // Create a signer (holds your keypair). You can also persist/import your own keys.
//! let signer = PubkySigner::new(Keypair::random())?;
//!
//! // Sign up on a homeserver (identified by its public key)
//! let homeserver = PublicKey::try_from("o4dksf...uyy").unwrap();
//! let agent = signer.signup_into_agent(&homeserver, None).await?;
//!
//! // Read/write using the drive API (session-scoped)
//! agent.drive().put("/pub/app/hello.txt", "hello").await?;
//! let body = agent.drive().get("/pub/app/hello.txt").await?.bytes().await?;
//! assert_eq!(&body, b"hello");
//!
//! // Unauthenticated read (public): user-qualified path, no session required
//! let public_drive = PubkyDrive::public()?;
//! let txt = public_drive
//!     .get(format!("{}/pub/app/hello.txt", signer.pubky()))
//!     .await?
//!     .text()
//!     .await?;
//! assert_eq!(txt, "hello");
//!
//! // Publish or resolve your homeserver `_pubky` (PkDNS/PKARR) record
//! signer.pkdns().publish_homeserver_if_stale(None).await?;
//! let resolved = PkDns::new()?.get_homeserver(&signer.pubky()).await;
//! println!("current homeserver: {:?}", resolved);
//!
//! // Keyless third-party app: start PubkyAuth and turn it into an agent
//! let capabilities = Capabilities::builder().write("/pub/pubky.app/").finish();
//! let (sub, url) = PubkyAuth::new(None, &capabilities)?.subscribe();  // None for default relay.
//! // display `url` via QR or deeplink it so the Signer can send the auth token.
//! // signer.send_auth_token(url);
//! let agent = sub.into_agent().await?; // session-bound agent
//! # Ok(()) }
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

// src/
// ├─ lib.rs                  -- Crate root: declares modules, re-exports public API (PubkyClient, PubkyAgent,
// │                             PubkyDrive, PubkyAuth, PkDns, PubkySigner, common types). Lints & crate docs.
// ├─ prelude.rs              -- Concentrated re-exports for quick-start imports.
// ├─ macros.rs               -- Cross-platform logging macro(s) (`cross_debug!`) used internally.
// ├─ errors.rs               -- Unified error types (Build/Request/Pkarr/Auth) + top-level Error/Result.
// ├─ util.rs                 -- Small internal utilities (e.g., `check_http_status` mapping non-2xx).
// ├─ global.rs               -- Process-wide, resettable `Arc<PubkyClient>` (lock-free loads with ArcSwap).
// │
// ├─ client/                 -- Stateless transport (“engine”): pkarr+DHT resolution and HTTP plumbing.
// │  ├─ core.rs              -- `PubkyClient` + `PubkyClientBuilder`: reqwest clients, pkarr client, defaults,
// │  │                         request timeout, user-agent, testnet toggles, max record age.
// │  ├─ http.rs              -- Request helper supporting `pubky://…` and pkarr-TLD HTTPS; ICANN routing on native.
// │  └─ http_targets/        -- Platform-specific glue for `cross_request` / URL transforms.
// │     ├─ native.rs         -- Native `cross_request` (delegates to `request`); `prepare_request` no-op.
// │     └─ wasm.rs           -- WASM `cross_request`/`prepare_request`: `pubky://` rewrite, pkarr endpoint resolution,
// │                             domain/port mapping (incl. testnet), `pubky-host` header injection.
// │
// ├─ agent/                  -- Stateful identity (“driver”): per-user session atop shared `PubkyClient`.
// │  ├─ core.rs              -- `PubkyAgent`: construct from `/session` responses, hold `Session`, expose `pubky()`,
// │  │                         `session()`, `client()`. (Its `drive()` is implemented in `drive/core.rs`.)
// │  └─ session.rs           -- Homeserver session helpers: `session_from_homeserver()`, `signout()`.
// │
// ├─ drive/                  -- Homeserver file/HTTP API (verbs + list + JSON) with agent/public modes.
// │  ├─ core.rs              -- `PubkyDrive`: session-mode (agent-scoped) vs public-mode (user-qualified paths);
// │  │                         URL resolution, cookie attachment (native). Also `impl PubkyAgent { fn drive(..) }`.
// │  ├─ http.rs              -- HTTP verbs: GET/HEAD (public or session), PUT/POST/PATCH/DELETE (session required).
// │  ├─ list.rs              -- `ListBuilder` for directory listings; returns absolute `Url`s.
// │  ├─ path.rs              -- `FilePath`, `PubkyPath`, and `IntoPubkyPath` conversions + parsing rules & tests.
// │  └─ json.rs              -- (feature `json`) `get_json`/`put_json` convenience helpers.
// │
// ├─ auth/                   -- Keyless app auth via HTTP relay (PubkyAuth).
// │  └─ flow.rs              -- `PubkyAuth`: build flow, subscribe (background polling), `AuthSubscription`,
// │                             token verification, `into_agent()`.
// │
// ├─ signer/                 -- High-level signer actor (holds keypair; can sign/publish/signup/signin).
// │  ├─ core.rs              -- `PubkySigner`: constructors (`new`/`with_client`/`random`), accessors (`pubky`, `keypair`).
// │  ├─ auth.rs              -- `send_auth_token(pubkyauth://…)` to an HTTP relay channel.
// │  └─ session.rs           -- `signup` / `signup_into_agent` / `into_agent` / `signin_and_publish`,
// │                             pkdns republish helpers (sync or background).
// │
// ├─ pkdns/                  -- PKDNS/PKARR actor for resolving & publishing `_pubky` records.
// │  └─ core.rs              -- `PkDns`: read-only (`new`/`with_client`) and publishing-capable
// │                             (`with_client_and_keypair`, or via `signer.pkdns()`); `get_homeserver`,
// │                             `publish_homeserver_{force,if_stale}`, internal host selection.
//
// Feature flags -- `json`: enables serde + JSON helpers in `drive/json.rs`.

mod agent;
mod auth;
mod client;
mod drive;
pub mod errors;
pub mod global;
mod macros;
mod pkdns;
mod signer;
mod util;

pub mod prelude;

// --- PUBLIC API EXPORTS ---
pub use agent::core::PubkyAgent;
pub use auth::PubkyAuth;
pub use client::core::{PubkyClient, PubkyClientBuilder};
pub use drive::core::PubkyDrive;
pub use errors::{BuildError, Error, Result};
pub use pkdns::core::PkDns;
pub use signer::PubkySigner;
// Export common types and constants
pub use crate::drive::{
    list::ListBuilder,
    path::{FilePath, PubkyPath},
};
// pub use agent::homeserver::ListBuilder;
pub use client::core::DEFAULT_RELAYS;
// Re-exports
pub use pkarr::{Keypair, PublicKey};
pub use pubky_common::{
    capabilities::{Capabilities, Capability},
    recovery_file,
    session::Session,
};
