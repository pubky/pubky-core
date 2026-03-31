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
//! - **Reads** use a best-effort pre-check (response size is unknown upfront),
//!   then count response body bytes as they stream through. If a client disconnects
//!   before consuming the full response, the uncounted tail bytes will not be charged.
//! - In-memory only: counters reset on server restart.
//! - Counters use `Relaxed` atomic ordering, which is sufficient for rate-limiting
//!   purposes (no need for happens-before guarantees between budget checks and
//!   unrelated memory accesses).

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

/// Tracks bytes used within a single time window for one direction (read or write).
#[derive(Debug)]
struct DirectionBudgetState {
    bytes_used: AtomicU64,
    window_start: Mutex<Instant>,
    /// Stored budget for config-change detection.
    budget: Mutex<Option<BandwidthBudget>>,
}

impl DirectionBudgetState {
    fn new(budget: Option<BandwidthBudget>) -> Self {
        Self {
            bytes_used: AtomicU64::new(0),
            window_start: Mutex::new(Instant::now()),
            budget: Mutex::new(budget),
        }
    }

    /// Check whether the window has expired or config has changed, resetting if so.
    /// Returns `(bytes_used, budget_bytes, seconds_remaining_in_window)`.
    fn check_and_maybe_reset(&self, budget: &BandwidthBudget) -> (u64, u64, u64) {
        let window_duration = budget.window_duration();
        let budget_bytes = budget.budget_bytes();

        let mut window_start = self.window_start.lock().unwrap();
        let elapsed = window_start.elapsed();

        // Check for config change
        let config_changed = {
            let mut stored = self.budget.lock().unwrap();
            let changed = stored.as_ref() != Some(budget);
            if changed {
                *stored = Some(budget.clone());
            }
            changed
        };

        // Reset window if expired or config changed
        if elapsed >= window_duration || config_changed {
            *window_start = Instant::now();
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
        let window_start = self.window_start.lock().unwrap();
        window_start.elapsed() > max_window
    }
}

/// Per-user budget state with independent read and write windows.
#[derive(Debug)]
struct UserBudgetState {
    read: DirectionBudgetState,
    write: DirectionBudgetState,
    /// Monotonic timestamp of last access, used for LRU-style eviction.
    last_accessed: Mutex<Instant>,
}

impl UserBudgetState {
    fn new(read_budget: Option<BandwidthBudget>, write_budget: Option<BandwidthBudget>) -> Self {
        Self {
            read: DirectionBudgetState::new(read_budget),
            write: DirectionBudgetState::new(write_budget),
            last_accessed: Mutex::new(Instant::now()),
        }
    }

    fn direction(&self, is_write: bool) -> &DirectionBudgetState {
        if is_write {
            &self.write
        } else {
            &self.read
        }
    }

    fn touch(&self) {
        *self.last_accessed.lock().unwrap() = Instant::now();
    }

    fn last_accessed(&self) -> Instant {
        *self.last_accessed.lock().unwrap()
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
            let mut interval = tokio::time::interval(Duration::from_secs(60));
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
/// Falls back to removing the least-recently-accessed entry if no stale
/// entries are found, preventing an attacker from resetting an arbitrary
/// active user's counters.
fn evict_stale_entries(budgets: &DashMap<PublicKey, Arc<UserBudgetState>>) {
    budgets.retain(|_, v| !v.is_expired(CLEANUP_EXPIRY));

    // If retain didn't free enough space, evict the least-recently-accessed entry.
    if budgets.len() >= MAX_TRACKED_USERS {
        let oldest = budgets
            .iter()
            .min_by_key(|entry| entry.value().last_accessed())
            .map(|entry| entry.key().clone());
        if let Some(key) = oldest {
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
            state.touch();

            let dir = state.direction(is_write);
            let (_bytes_used, budget_bytes, seconds_remaining) =
                dir.check_and_maybe_reset(budget);

            if is_write {
                // Determine write cost upfront for atomic reservation.
                let content_length = req
                    .headers()
                    .get(axum::http::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                let (reserve_cost, fully_reserved) = match content_length {
                    // Known size: reserve the full cost atomically.
                    Some(cl) => (cl.max(MIN_WRITE_COST_BYTES), true),
                    // Unknown size: reserve the minimum; streaming will add more.
                    None => (MIN_WRITE_COST_BYTES, false),
                };

                if dir.try_reserve(reserve_cost, budget_bytes).is_err() {
                    let retry_after = seconds_remaining.max(1);
                    tracing::debug!(
                        "Per-user bandwidth budget exceeded for {} (write, budget={}B, retry_after={}s)",
                        pubkey.z32(), budget_bytes, retry_after,
                    );
                    return Ok(budget_exceeded_response(retry_after));
                }

                count_write_bytes(req, &state, &mut inner, fully_reserved).await
            } else {
                // Reads: pre-check current usage (can't know response size upfront).
                let bytes_used = dir.bytes_used.load(Ordering::Relaxed);
                if bytes_used >= budget_bytes {
                    let retry_after = seconds_remaining.max(1);
                    tracing::debug!(
                        "Per-user bandwidth budget exceeded for {} (read, used={}B, budget={}B, retry_after={}s)",
                        pubkey.z32(), bytes_used, budget_bytes, retry_after,
                    );
                    return Ok(budget_exceeded_response(retry_after));
                }
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

        // Exhaust the read budget
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

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

        // Read should still work (independent budget, reads count response bytes not request bytes)
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        // Read passes the pre-check because the handler returns an empty body,
        // so bytes_used stays at 0 for reads (only response body bytes are counted).
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

        // First GET: the response body is 2048 bytes, which exceeds the 1kb budget.
        // The request passes the pre-check (0 < 1024) but the response body
        // counting pushes bytes_used to 2048.
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Must consume the body so the counting stream runs.
        let _ = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();

        // Second GET: pre-check sees 2048 >= 1024 → 429.
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
}
