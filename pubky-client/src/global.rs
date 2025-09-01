//! Global, resettable `PubkyClient` for lazy construction of high-level actors.
//!
//! # Why this exists
//! Most applications want easy, zero-setup construction of `PubkyAgent` (session actor),
//! `PubkySigner` (key holder), and `PubkyAuth` without having to plumb a
//! `PubkyClient` everywhere. This module provides a process-wide, lazily initialized,
//! resettable client that those “lazy constructors” can reuse. The goals:
//!
//! - **Ergonomics**: one-liners like `PubkySigner:new(kp)` and
//!   `PubkyAuth::new(..)`.
//! - **Performance**: reuse a single transport stack (connection pools, TLS state, pkarr cache).
//! - **Safety**: resetting the global must not invalidate already-constructed agents.
//!
//! # Design
//! - Backing storage is `ArcSwapOption<PubkyClient>` inside a `OnceLock`.
//! - **Reads are lock-free**; fetching the global is a single atomic load +
//!   cheap `Arc` clone.
//! - **Reset is safe**; `set_client`/`drop_client` publish a new `Arc` (or `None`).
//!   Existing `Arc<PubkyClient>` handles keep the old client alive until dropped.
//! - **Init is fallible** and returns `BuildError` instead of panicking.
//!
//! # When to use
//! - Apps that prefer convenience over explicit dependency injection, CLIs,
//!   tests, examples, etc.
//! - Libraries that offer “lazy” constructors for DX while still exposing
//!   explicit constructors that accept `Arc<PubkyClient>`.
//!
//! # When not to use
//! - Long-lived services that manage multiple client configurations; pass
//!   an explicit `Arc<PubkyClient>` to constructors instead.
//!
//! # Concurrency and races
//! - If multiple threads call `global_client()` concurrently before initialization,
//!   more than one `PubkyClient` may be constructed; the last stored wins and
//!   losers are dropped immediately. This is acceptable and uncommon.
//!
//! # Test hygiene
//! - Use `drop_client()` between tests to guarantee a fresh default client,
//!   or use `set_client(..)` to inject a deterministic one (for example a Testnet
//!   configured client).
//!
//! # Examples
//! Fetch the default client (lazily created):
//! ```rust
//! # use pubky::{global, PubkyClient};
//! let client = global::global_client()?;
//! // Reused on subsequent calls:
//! let same = global::global_client()?;
//! assert!(Arc::ptr_eq(&client, &same));
//! # Ok::<(), pubky::BuildError>(())
//! ```
//!
//! Override globally:
//! ```rust
//! # use pubky::{global, PubkyClient};
//! # use std::sync::Arc;
//! let custom = Arc::new(PubkyClient::builder().build()?);
//! global::set_client(custom.clone());
//! assert!(Arc::ptr_eq(&custom, &global::global_client()?));
//! # Ok::<(), pubky::BuildError>(())
//! ```
//!
//! Reset to “no client”; next call re-initializes lazily:
//! ```rust
//! # use pubky::global;
//! global::drop_client();
//! let _fresh = global::global_client()?;
//! # Ok::<(), pubky::BuildError>(())
//! ```

use arc_swap::ArcSwapOption;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::{BuildError, PubkyClient};

/// Process-wide slot for the optional default client.
///
/// Initialized on first use; may be replaced or cleared at runtime.
/// Lock-free loads; last-writer-wins stores.
static GLOBAL_CLIENT: OnceLock<ArcSwapOption<PubkyClient>> = OnceLock::new();

#[inline]
fn slot() -> &'static ArcSwapOption<PubkyClient> {
    GLOBAL_CLIENT.get_or_init(|| ArcSwapOption::from(None))
}

/// Get-or-init the global default `PubkyClient`.
///
/// - Returns an `Arc<PubkyClient>` backed by an atomically published instance.
/// - Constructs a new client with `PubkyClient::new()` on first use if none is present.
/// - Never invalidates previously returned `Arc`s; they keep the old client alive.
///
/// # Errors
/// Returns `BuildError` if constructing a new default client fails.
pub fn global_client() -> Result<Arc<PubkyClient>, BuildError> {
    if let Some(existing) = slot().load_full() {
        return Ok(existing);
    }
    let candidate = Arc::new(PubkyClient::new()?);
    // Last write wins if racy; losing candidate is dropped.
    slot().store(Some(candidate.clone()));
    Ok(slot().load_full().expect("client was just stored"))
}

/// Replace the global default client.
///
/// Publishes `new_client` atomically. Existing handles continue to use the
/// previous client until they are dropped.
pub fn set_client(new_client: Arc<PubkyClient>) {
    slot().store(Some(new_client));
}

/// Clear the global default client.
///
/// After this call, the next `global_client()` will lazily construct a fresh client.
/// Existing `Arc<PubkyClient>` handles remain valid.
pub fn drop_client() {
    slot().store(None);
}
