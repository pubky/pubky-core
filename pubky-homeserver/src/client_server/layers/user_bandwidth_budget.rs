//! Per-user bandwidth budget middleware.
//!
//! Reads the resolved `UserLimitConfig` from request extensions (set by `UserLimitResolverLayer`)
//! and enforces per-user read/write bandwidth budgets using simple atomic counters with
//! time-windowed resets. Separate from the global path-based `RateLimiterLayer`.
//!
//! **Design:**
//! - Only applies to **authenticated** requests (checks for `AuthenticatedSession`
//!   marker). Anonymous reads of public data are not counted — external scrapers
//!   are handled by the global IP-based `RateLimiterLayer` instead.
//! - **Writes** use atomic `fetch_add` + rollback: the estimated cost is reserved
//!   atomically before the request proceeds. If Content-Length is present, the full
//!   cost is reserved; otherwise `MIN_WRITE_COST_BYTES` is reserved and streaming
//!   chunks add on top. This prevents concurrent requests from all passing a
//!   stale pre-check before any deduction is visible.
//! - **Reads** use the same atomic `try_reserve` pattern as writes: a minimum
//!   cost ([`MIN_READ_COST_BYTES`]) is reserved atomically before the request
//!   proceeds, then actual response body bytes are counted as they stream
//!   through. This prevents concurrent reads from all passing a stale
//!   pre-check before any deduction is visible.
//! - In-memory only: counters reset on server restart.

use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::Poll;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::IntoResponse;
use dashmap::DashMap;
use futures_util::future::BoxFuture;
use futures_util::StreamExt;
use pubky_common::crypto::PublicKey;
use tower::{Layer, Service};

use crate::client_server::extractors::PubkyHost;
use crate::client_server::layers::authz::AuthenticatedSession;
use crate::data_directory::quota_config::BandwidthBudget;
use crate::data_directory::user_limit_config::UserLimitConfig;
use crate::shared::HttpError;

/// Window state protected by a single mutex to avoid split-lock inconsistency.
#[derive(Debug)]
struct WindowState {
    window_start: Instant,
    budget: Option<BandwidthBudget>,
}

/// Tracks bytes used within a single time window for one direction (read or write).
#[derive(Debug)]
struct DirectionBudgetState {
    bytes_used: AtomicU64,
    /// Window start and stored budget config, guarded by a single lock to prevent
    /// inconsistent reads when both are checked/reset together.
    window: Mutex<WindowState>,
}

impl DirectionBudgetState {
    fn new(budget: Option<BandwidthBudget>) -> Self {
        Self {
            bytes_used: AtomicU64::new(0),
            window: Mutex::new(WindowState {
                window_start: Instant::now(),
                budget,
            }),
        }
    }

    /// Check whether the window has expired or config has changed, resetting if so.
    /// Returns `(bytes_used, budget_bytes, seconds_remaining_in_window)`.
    fn check_and_maybe_reset(&self, budget: &BandwidthBudget) -> (u64, u64, u64) {
        let window_duration = budget.window_duration();
        let budget_bytes = budget.budget_bytes();

        let mut state = self.window.lock().unwrap();
        let elapsed = state.window_start.elapsed();

        // Check for config change
        let config_changed = state.budget.as_ref() != Some(budget);
        if config_changed {
            state.budget = Some(budget.clone());
        }

        // Reset window if expired or config changed
        if elapsed >= window_duration || config_changed {
            state.window_start = Instant::now();
            self.bytes_used.store(0, Ordering::Relaxed);
            return (0, budget_bytes, window_duration.as_secs());
        }

        let seconds_remaining = window_duration.as_secs().saturating_sub(elapsed.as_secs());

        let used = self.bytes_used.load(Ordering::Relaxed);
        (used, budget_bytes, seconds_remaining)
    }

    fn add_bytes(&self, bytes: u64) {
        self.bytes_used.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Atomically reserve `bytes` from the budget using `fetch_add`.
    ///
    /// Returns `Ok(())` if the budget was not yet exhausted before this
    /// reservation (i.e. `previous < budget_bytes`). The request that *crosses*
    /// the boundary is allowed through (soft limit), but all subsequent requests
    /// are rejected until the window resets. This prevents concurrent requests
    /// from all passing a stale pre-check before any deduction is visible.
    ///
    /// Returns `Err(())` after rolling back the reservation if the budget was
    /// already exhausted (`previous >= budget_bytes`).
    fn try_reserve(&self, bytes: u64, budget_bytes: u64) -> Result<(), ()> {
        let previous = self.bytes_used.fetch_add(bytes, Ordering::Relaxed);
        if previous >= budget_bytes {
            // Budget was already exhausted before this request — roll back.
            self.bytes_used.fetch_sub(bytes, Ordering::Relaxed);
            Err(())
        } else {
            // Budget had room (even if this request pushes it over).
            // The overshoot is bounded by one request's cost.
            Ok(())
        }
    }

    fn is_expired(&self, max_window: Duration) -> bool {
        let state = self.window.lock().unwrap();
        state.window_start.elapsed() > max_window
    }
}

/// Per-user budget state with independent read and write windows.
#[derive(Debug)]
struct UserBudgetState {
    read: DirectionBudgetState,
    write: DirectionBudgetState,
}

impl UserBudgetState {
    fn new(read_budget: Option<BandwidthBudget>, write_budget: Option<BandwidthBudget>) -> Self {
        Self {
            read: DirectionBudgetState::new(read_budget),
            write: DirectionBudgetState::new(write_budget),
        }
    }

    fn direction(&self, is_write: bool) -> &DirectionBudgetState {
        if is_write {
            &self.write
        } else {
            &self.read
        }
    }

    /// Returns true if both direction windows have expired, meaning this entry can be evicted.
    fn is_expired(&self, max_window: Duration) -> bool {
        self.read.is_expired(max_window) && self.write.is_expired(max_window)
    }
}

/// Maximum number of distinct users tracked in the per-user budget map.
const MAX_TRACKED_USERS: usize = 100_000;

/// Duration after which idle entries are eligible for cleanup.
const CLEANUP_EXPIRY: Duration = Duration::from_secs(24 * 60 * 60); // 1 day

/// How often the background task runs to evict expired entries.
const CLEANUP_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct UserBandwidthBudgetLayer {
    budgets: Arc<DashMap<PublicKey, Arc<UserBudgetState>>>,
}

impl UserBandwidthBudgetLayer {
    pub fn new() -> Self {
        let budgets: Arc<DashMap<PublicKey, Arc<UserBudgetState>>> = Arc::new(DashMap::new());

        // Periodic cleanup: remove entries whose windows have expired.
        let budgets_weak: Weak<DashMap<PublicKey, Arc<UserBudgetState>>> = Arc::downgrade(&budgets);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                let Some(budgets_ref) = budgets_weak.upgrade() else {
                    break;
                };
                budgets_ref.retain(|_, v| !v.is_expired(CLEANUP_EXPIRY));
            }
        });

        Self { budgets }
    }
}

impl<S> Layer<S> for UserBandwidthBudgetLayer {
    type Service = UserBandwidthBudgetMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        UserBandwidthBudgetMiddleware {
            inner,
            budgets: self.budgets.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserBandwidthBudgetMiddleware<S> {
    inner: S,
    budgets: Arc<DashMap<PublicKey, Arc<UserBudgetState>>>,
}

/// Returns true if the HTTP method is a "write" operation.
fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Evict stale entries from the budget map when it reaches capacity.
/// Falls back to removing a random entry if no stale entries are found,
/// avoiding an O(n) scan that could cause latency spikes under load.
fn evict_stale_entries(budgets: &DashMap<PublicKey, Arc<UserBudgetState>>) {
    budgets.retain(|_, v| !v.is_expired(CLEANUP_EXPIRY));

    // If retain didn't free enough space, evict a random entry.
    // O(1) — just grabs the first entry from an arbitrary shard.
    if budgets.len() >= MAX_TRACKED_USERS {
        let arbitrary_key = budgets.iter().next().map(|entry| entry.key().clone());
        if let Some(key) = arbitrary_key {
            budgets.remove(&key);
        }
    }
}

/// Build a 429 response with a Retry-After header.
fn budget_exceeded_response(retry_after_secs: u64) -> axum::response::Response {
    let mut response = HttpError::new_with_message(
        StatusCode::TOO_MANY_REQUESTS,
        "Per-user bandwidth budget exceeded",
    )
    .into_response();
    response.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        axum::http::HeaderValue::from(retry_after_secs),
    );
    response
}

/// Minimum bytes charged per write request, even if the body is empty.
/// Ensures bodyless mutations (e.g. DELETE) still consume budget.
const MIN_WRITE_COST_BYTES: u64 = 256;

/// Minimum bytes reserved atomically per read request before the response
/// size is known. Actual response body bytes are counted on top as they
/// stream through. This prevents concurrent reads from all passing a stale
/// pre-check before any deduction is visible.
const MIN_READ_COST_BYTES: u64 = 256;

/// Deduct write bytes from the budget after the pre-check has passed.
///
/// If Content-Length is present, the cost was already reserved atomically by
/// `try_reserve` in the caller — nothing more to do. Otherwise wraps the body
/// stream to count bytes as chunks flow through, then ensures the total charge
/// is at least [`MIN_WRITE_COST_BYTES`] (for bodyless mutations like DELETE).
async fn count_write_bytes<S>(
    req: Request<Body>,
    state: &Arc<UserBudgetState>,
    inner: &mut S,
    already_reserved: bool,
) -> Result<axum::response::Response, Infallible>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>,
{
    if already_reserved {
        // Content-Length path: cost was atomically reserved by try_reserve.
        return inner.call(req).await;
    }

    // No Content-Length: MIN_WRITE_COST_BYTES was reserved by try_reserve.
    // Stream-count additional bytes on top.
    let (parts, body) = req.into_parts();
    let state_clone = state.clone();
    let counted_stream = body.into_data_stream().map(move |chunk| {
        if let Ok(ref bytes) = chunk {
            state_clone.write.add_bytes(bytes.len() as u64);
        }
        chunk
    });
    let new_req = Request::from_parts(parts, Body::from_stream(counted_stream));
    inner.call(new_req).await
}

/// Wrap the response body to count bytes read as they stream out.
///
/// `MIN_READ_COST_BYTES` was already reserved atomically by `try_reserve`
/// in the caller. Response body bytes are counted on top as they stream
/// through, so the total charge is `MIN_READ_COST_BYTES + response_body_bytes`.
async fn count_read_bytes<S>(
    req: Request<Body>,
    state: &Arc<UserBudgetState>,
    inner: &mut S,
) -> Result<axum::response::Response, Infallible>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>,
{
    let response = inner.call(req).await?;
    let (parts, body) = response.into_parts();
    let state_clone = state.clone();
    let counted_stream = body.into_data_stream().map(move |chunk| {
        if let Ok(ref bytes) = chunk {
            state_clone.read.add_bytes(bytes.len() as u64);
        }
        chunk
    });
    Ok(axum::response::Response::from_parts(
        parts,
        Body::from_stream(counted_stream),
    ))
}

impl<S> Service<Request<Body>> for UserBandwidthBudgetMiddleware<S>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
        + Send
        + 'static
        + Clone,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let budgets = self.budgets.clone();

        Box::pin(async move {
            let is_authenticated = req.extensions().get::<AuthenticatedSession>().is_some();
            let pubky_host = req.extensions().get::<PubkyHost>().cloned();
            let user_limits = req.extensions().get::<UserLimitConfig>().cloned();

            let (true, Some(pubky_host), Some(limits)) =
                (is_authenticated, pubky_host, user_limits)
            else {
                return inner.call(req).await;
            };

            let pubkey = pubky_host.public_key().clone();
            let is_write = is_write_method(req.method());
            let budget = if is_write {
                limits.rate_write.as_ref()
            } else {
                limits.rate_read.as_ref()
            };

            let Some(budget) = budget else {
                return inner.call(req).await;
            };

            if !budgets.contains_key(&pubkey) && budgets.len() >= MAX_TRACKED_USERS {
                evict_stale_entries(&budgets);
            }

            let state = budgets
                .entry(pubkey.clone())
                .or_insert_with(|| {
                    Arc::new(UserBudgetState::new(
                        limits.rate_read.clone(),
                        limits.rate_write.clone(),
                    ))
                })
                .clone();
            let dir = state.direction(is_write);
            let (_bytes_used, budget_bytes, seconds_remaining) = dir.check_and_maybe_reset(budget);

            let direction_label = if is_write { "write" } else { "read" };
            let reserve_cost = if is_write {
                // Determine write cost upfront for atomic reservation.
                let content_length = req
                    .headers()
                    .get(axum::http::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());
                content_length
                    .unwrap_or(MIN_WRITE_COST_BYTES)
                    .max(MIN_WRITE_COST_BYTES)
            } else {
                MIN_READ_COST_BYTES
            };

            if dir.try_reserve(reserve_cost, budget_bytes).is_err() {
                let retry_after = seconds_remaining.max(1);
                tracing::debug!(
                    "Per-user bandwidth budget exceeded for {} ({direction_label}, budget={}B, retry_after={}s)",
                    pubkey.z32(), budget_bytes, retry_after,
                );
                return Ok(budget_exceeded_response(retry_after));
            }

            if is_write {
                let fully_reserved = req
                    .headers()
                    .get(axum::http::header::CONTENT_LENGTH)
                    .is_some();
                count_write_bytes(req, &state, &mut inner, fully_reserved).await
            } else {
                count_read_bytes(req, &state, &mut inner).await
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum::http::{Method, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;
    use pubky_common::crypto::Keypair;
    use tower::ServiceExt;

    use crate::client_server::extractors::PubkyHost;
    use crate::client_server::layers::authz::AuthenticatedSession;
    use crate::data_directory::quota_config::BandwidthBudget;
    use crate::data_directory::user_limit_config::UserLimitConfig;

    use super::*;

    fn budget(s: &str) -> Option<BandwidthBudget> {
        Some(BandwidthBudget::from_str(s).unwrap())
    }

    async fn ok_handler() -> impl IntoResponse {
        StatusCode::OK
    }

    fn test_app() -> Router {
        Router::new()
            .route("/test", get(ok_handler).post(ok_handler).delete(ok_handler))
            .layer(UserBandwidthBudgetLayer::new())
    }

    fn make_request(
        method: Method,
        pubkey: &pubky_common::crypto::PublicKey,
        limits: &UserLimitConfig,
    ) -> Request<Body> {
        let mut req = Request::builder()
            .method(method)
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));
        req.extensions_mut().insert(limits.clone());
        req.extensions_mut().insert(AuthenticatedSession);
        req
    }

    fn make_request_with_body(
        method: Method,
        pubkey: &pubky_common::crypto::PublicKey,
        limits: &UserLimitConfig,
        body: Vec<u8>,
    ) -> Request<Body> {
        let len = body.len();
        let mut req = Request::builder()
            .method(method)
            .uri("/test")
            .header("content-length", len.to_string())
            .body(Body::from(body))
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));
        req.extensions_mut().insert(limits.clone());
        req.extensions_mut().insert(AuthenticatedSession);
        req
    }

    #[tokio::test]
    async fn test_budget_write_enforced() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        // 1kb/m budget = 1024 bytes per minute
        let limits = UserLimitConfig {
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        // First request: 1024 bytes uses up the full budget (may overshoot — acceptable)
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &limits,
                vec![0; 1024],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second request: pre-check sees 1024 >= 1024 → 429
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &limits,
                vec![0; 512],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_no_pubky_host_passes_through() {
        let app = test_app();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_no_rate_config_passes_through() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig::default();

        for _ in 0..10 {
            let resp = app
                .clone()
                .oneshot(make_request(Method::GET, &pubkey, &limits))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn test_read_and_write_budgets_independent() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: budget("1kb/m"),
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        // Exhaust write budget with a large body
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &limits,
                vec![0; 2048],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Write should be exhausted now
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &limits,
                vec![0; 1],
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Write budget should be exhausted"
        );

        // Read should still work (independent budget). Each read reserves
        // MIN_READ_COST_BYTES (256) atomically. The handler returns an empty
        // body, so total cost per read is 256 bytes. With a 1kb budget we
        // can do 4 reads (4 × 256 = 1024).
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Read budget should be independent of write"
        );
    }

    #[tokio::test]
    async fn test_different_users_have_separate_budgets() {
        let app = test_app();
        let pubkey1 = Keypair::random().public_key();
        let pubkey2 = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        // Exhaust user1's write budget
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey1,
                &limits,
                vec![0; 2048],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey1,
                &limits,
                vec![0; 512],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // User2 should still be allowed
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey2,
                &limits,
                vec![0; 512],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_retry_after_header_present() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        // Exhaust the budget
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &limits,
                vec![0; 2048],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second request should return 429 with Retry-After header
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &limits,
                vec![0; 512],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .expect("429 response should include Retry-After header");
        let secs: u64 = retry_after
            .to_str()
            .unwrap()
            .parse()
            .expect("Retry-After should be a number");
        assert!(secs > 0, "Retry-After should be at least 1 second");
    }

    #[tokio::test]
    async fn test_budget_change_resets_counters() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();

        // Start with a tight budget
        let tight_limits = UserLimitConfig {
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        // Exhaust the tight budget
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &tight_limits,
                vec![0; 2048],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &tight_limits,
                vec![0; 512],
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // Admin loosens to 1mb/m — counters should reset, allowing requests again
        let loose_limits = UserLimitConfig {
            rate_write: budget("1mb/m"),
            ..Default::default()
        };
        let resp = app
            .clone()
            .oneshot(make_request_with_body(
                Method::POST,
                &pubkey,
                &loose_limits,
                vec![0; 512],
            ))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Budget change should reset counters, allowing new requests"
        );
    }

    #[tokio::test]
    async fn test_delete_counts_minimum_cost() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        // Budget just under 4 × MIN_WRITE_COST_BYTES (4 × 256 = 1024 = 1kb)
        let limits = UserLimitConfig {
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        // Each DELETE has no body but still costs MIN_WRITE_COST_BYTES (256).
        // 4 DELETEs = 4 × 256 = 1024 bytes = exactly the 1kb budget.
        for i in 0..4 {
            let resp = app
                .clone()
                .oneshot(make_request(Method::DELETE, &pubkey, &limits))
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "DELETE #{} should succeed",
                i
            );
        }

        // 5th DELETE should be rejected — budget exhausted
        let resp = app
            .clone()
            .oneshot(make_request(Method::DELETE, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "DELETE should be rate-limited after budget is exhausted"
        );
    }

    #[tokio::test]
    async fn test_read_budget_counts_response_bytes() {
        // Build an app whose handler returns a known-size body.
        async fn big_handler() -> impl IntoResponse {
            (StatusCode::OK, vec![0u8; 2048])
        }

        let app = Router::new()
            .route("/test", get(big_handler))
            .layer(UserBandwidthBudgetLayer::new());

        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: budget("1kb/m"),
            ..Default::default()
        };

        // First GET: atomically reserves MIN_READ_COST_BYTES (256), then the
        // response body adds 2048 bytes via streaming. Total charge: 2304 bytes,
        // which exceeds the 1kb (1024) budget.
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Must consume the body so the counting stream runs.
        let _ = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();

        // Second GET: try_reserve sees 2304 >= 1024 → rolls back → 429.
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Read budget should be exhausted after large response"
        );
    }

    #[tokio::test]
    async fn test_unauthenticated_request_bypasses_budget() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_write: budget("1kb/m"),
            ..Default::default()
        };

        let make_unauthed = |method: Method| {
            let mut req = Request::builder()
                .method(method)
                .uri("/test")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(PubkyHost(pubkey.clone()));
            req.extensions_mut().insert(limits.clone());
            // No AuthenticatedSession inserted
            req
        };

        // Many unauthenticated requests should all pass through
        for _ in 0..10 {
            let resp = app
                .clone()
                .oneshot(make_unauthed(Method::POST))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn test_read_budget_atomic_reservation_limits_concurrent_reads() {
        // Each read reserves MIN_READ_COST_BYTES (256) atomically.
        // With a 1kb budget (1024 bytes), at most 4 reads can pass the
        // atomic reservation (4 × 256 = 1024). The 5th must be rejected.
        let app = test_app(); // handler returns empty body
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: budget("1kb/m"),
            ..Default::default()
        };

        for i in 0..4 {
            let resp = app
                .clone()
                .oneshot(make_request(Method::GET, &pubkey, &limits))
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Read #{i} should succeed (within budget)"
            );
        }

        // 5th read: budget exhausted (4 × 256 = 1024 >= 1024) → 429
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Read should be rejected after budget exhausted by atomic reservations"
        );
    }

    #[tokio::test]
    async fn test_evict_stale_entries_removes_expired() {
        let budgets: DashMap<pubky_common::crypto::PublicKey, Arc<UserBudgetState>> =
            DashMap::new();

        let pk1 = Keypair::random().public_key();
        let pk2 = Keypair::random().public_key();
        let pk3 = Keypair::random().public_key();

        // pk1: expired — manually set window_start to the past
        let state1 = Arc::new(UserBudgetState::new(None, None));
        {
            let mut ws = state1.read.window.lock().unwrap();
            ws.window_start = Instant::now() - CLEANUP_EXPIRY - Duration::from_secs(1);
        }
        {
            let mut ws = state1.write.window.lock().unwrap();
            ws.window_start = Instant::now() - CLEANUP_EXPIRY - Duration::from_secs(1);
        }
        budgets.insert(pk1.clone(), state1);

        // pk2: fresh — should survive
        budgets.insert(pk2.clone(), Arc::new(UserBudgetState::new(None, None)));

        // pk3: only read expired, write still fresh — should survive
        let state3 = Arc::new(UserBudgetState::new(None, None));
        {
            let mut ws = state3.read.window.lock().unwrap();
            ws.window_start = Instant::now() - CLEANUP_EXPIRY - Duration::from_secs(1);
        }
        budgets.insert(pk3.clone(), state3);

        evict_stale_entries(&budgets);

        assert!(
            !budgets.contains_key(&pk1),
            "Fully expired entry should be evicted"
        );
        assert!(
            budgets.contains_key(&pk2),
            "Fresh entry should survive eviction"
        );
        assert!(
            budgets.contains_key(&pk3),
            "Partially expired entry should survive eviction"
        );
    }

    #[tokio::test]
    async fn test_authenticated_read_counts_response_bytes_through_middleware() {
        // Verifies that authenticated GET requests have their response bytes
        // counted against the read budget by the full middleware chain.
        async fn handler_2kb() -> impl IntoResponse {
            (StatusCode::OK, vec![0u8; 2048])
        }

        let layer = UserBandwidthBudgetLayer::new();
        let app = Router::new()
            .route("/test", get(handler_2kb))
            .layer(layer.clone());

        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: budget("1kb/m"),
            ..Default::default()
        };

        // First authenticated read: atomically reserves MIN_READ_COST_BYTES
        // (256), then the response body adds 2048 bytes. Total: 2304 > 1024.
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Must consume the body so the counting stream runs.
        let _ = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();

        // Second authenticated read: try_reserve sees 2304 >= 1024 → 429
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Authenticated read should be rate-limited after response body bytes exceed budget"
        );

        // Verify that an unauthenticated read is NOT affected
        let mut unauthed_req = Request::builder()
            .method(Method::GET)
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        unauthed_req
            .extensions_mut()
            .insert(PubkyHost(pubkey.clone()));
        unauthed_req.extensions_mut().insert(limits.clone());
        // No AuthenticatedSession marker
        let resp = app.clone().oneshot(unauthed_req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Unauthenticated read should bypass budget"
        );
    }
}
