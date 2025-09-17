//! Global, resettable `PubkyHttpClient` for lazy construction of high-level actors.
//!
//! # Why this exists
//! Most applications want easy, zero-setup construction of `PubkySession` (session actor),
//! `PubkySigner` (key holder), and `PubkyAuth` without passing a `PubkyHttpClient` everywhere.
//! This module provides a process-wide, lazily initialized, resettable client that those
//! “lazy constructors” can reuse.
//!
//! - **Ergonomics:** one-liners like `PubkySigner::new(kp)` and `PubkyAuth::new(..)` just work.
//! - **Performance:** reuse a single transport stack (connection pools, TLS state, pkarr cache).
//! - **Safety:** resetting the global must not invalidate already-constructed clients/sessions.
//!
//! # Design
//! - Backing storage is `ArcSwapOption<PubkyHttpClient>` inside a `OnceLock`.
//! - **Reads are lock-free**; `global_client()` does a single atomic load and returns a **cheap clone**
//!   of the current `PubkyHttpClient`.
//! - **Reset is safe**; `set_client`/`drop_client` publish a new instance (or `None`). Existing clones
//!   keep working independently.
//! - **Init is fallible** and returns `BuildError` instead of panicking.
//!
//! # When to use
//! - Apps that prefer convenience over explicit dependency injection (CLIs, examples, tests).
//! - Libraries that offer “lazy” constructors for DX while still exposing explicit constructors
//!   that accept a `PubkyHttpClient`.
//!
//! # When not to use
//! - Long-lived services that manage multiple client configurations; pass an explicit
//!   `PubkyHttpClient` to constructors instead.
//!
//! # Concurrency and races
//! - If multiple threads call `global_client()` concurrently before initialization, more than one
//!   `PubkyHttpClient` may be constructed; the last stored wins and the others are dropped. This
//!   is acceptable and uncommon.
//!
//! # Test hygiene
//! - Use `drop_client()` between tests to guarantee a fresh default client, or `set_client(..)`
//!   to inject a deterministic one (e.g., a Testnet-configured client).
//!
//! # Examples
//! Fetch the default client (lazily created):
//! ```
//! # use pubky::{global_client, PubkyHttpClient};
//! let client = global_client()?;
//! // Subsequent calls return cheap clones of the same underlying configuration:
//! let same_again = global_client()?;
//! # Ok::<(), pubky::BuildError>(())
//! ```
//!
//! Override globally:
//! ```
//! # use pubky::{global_client, set_global_client, PubkyHttpClient};
//! let custom = PubkyHttpClient::builder().build()?;
//! set_global_client(custom);
//! // New calls now clone the custom client:
//! let now_custom = global_client()?;
//! # Ok::<(), pubky::BuildError>(())
//! ```
//!
//! Reset to “no client”; next call re-initializes lazily:
//! ```
//! # use pubky::drop_global_client;
//! drop_global_client();
//! let fresh = pubky::global_client()?; // constructed on demand
//! # Ok::<(), pubky::BuildError>(())
//! ```

use arc_swap::ArcSwapOption;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::{BuildError, PubkyHttpClient};

/// Process-wide slot for the optional default client.
///
/// Initialized on first use; may be replaced or cleared at runtime.
/// Lock-free loads; last-writer-wins stores.
static GLOBAL_CLIENT: OnceLock<ArcSwapOption<PubkyHttpClient>> = OnceLock::new();

#[inline]
fn slot() -> &'static ArcSwapOption<PubkyHttpClient> {
    GLOBAL_CLIENT.get_or_init(|| ArcSwapOption::from(None))
}

/// Get-or-init the process-wide default client.
///
/// Returns a **clone** of the current default `PubkyHttpClient`.
/// Clones are cheap and keep their own internal handles (reqwest pools, pkarr client).
///
/// - On first use, constructs via `PubkyHttpClient::new()`.
/// - Subsequent calls are lock-free and just clone the current instance.
/// - Clones remain valid even if you later call `set_client` or `drop_client`.
pub fn global_client() -> Result<PubkyHttpClient, BuildError> {
    if let Some(current) = slot().load_full() {
        // Clone the inner client; dropping this Arc only decrements the refcount.
        return Ok(current.as_ref().clone());
    }

    // Initialize a fresh one and publish it, racing safely with other initializers.
    let fresh = PubkyHttpClient::new()?;
    slot().store(Some(Arc::new(fresh.clone())));
    Ok(fresh)
}

/// Replace the global default client.
///
/// Publishes `new_client` atomically. Existing handles continue to use the
/// previous client until they are dropped.
pub fn set_global_client(new_client: PubkyHttpClient) {
    slot().store(Some(Arc::new(new_client)));
}
/// Clear the global default client.
///
/// After this call, the next `global_client()` will lazily construct a fresh client.
/// Existing `Arc<PubkyHttpClient>` handles remain valid.
pub fn drop_global_client() {
    slot().store(None);
}
