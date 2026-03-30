//! Per-user bandwidth budget middleware.
//!
//! Reads the resolved `UserLimitConfig` from request extensions (set by `UserLimitResolverLayer`)
//! and enforces per-user read/write bandwidth budgets using simple atomic counters with
//! time-windowed resets. Separate from the global path-based `RateLimiterLayer`.
//!
//! **Design:**
//! - Only applies to **authenticated** requests (checks for `AuthenticatedSession` marker).
//! - Pre-checks `bytes_used < budget_bytes` at request start → 429 if exceeded.
//! - Deducts actual bytes after the request completes (wraps request/response body streams).
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

        let seconds_remaining = window_duration
            .as_secs()
            .saturating_sub(elapsed.as_secs());

        let used = self.bytes_used.load(Ordering::Relaxed);
        (used, budget_bytes, seconds_remaining)
    }

    fn add_bytes(&self, bytes: u64) {
        self.bytes_used.fetch_add(bytes, Ordering::Relaxed);
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
const CLEANUP_EXPIRY: Duration = Duration::from_secs(86400); // 1 day

#[derive(Debug, Clone)]
pub struct UserRateLimiterLayer {
    budgets: Arc<DashMap<PublicKey, Arc<UserBudgetState>>>,
}

impl UserRateLimiterLayer {
    pub fn new() -> Self {
        let budgets: Arc<DashMap<PublicKey, Arc<UserBudgetState>>> = Arc::new(DashMap::new());

        // Periodic cleanup: remove entries whose windows have expired.
        let budgets_weak: Weak<DashMap<PublicKey, Arc<UserBudgetState>>> =
            Arc::downgrade(&budgets);
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

impl<S> Layer<S> for UserRateLimiterLayer {
    type Service = UserRateLimiterMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        UserRateLimiterMiddleware {
            inner,
            budgets: self.budgets.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRateLimiterMiddleware<S> {
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
fn evict_stale_entries(budgets: &DashMap<PublicKey, Arc<UserBudgetState>>) {
    budgets.retain(|_, v| !v.is_expired(CLEANUP_EXPIRY));

    // If retain didn't free enough space, forcibly remove an arbitrary entry.
    if budgets.len() >= MAX_TRACKED_USERS {
        if let Some(entry) = budgets.iter().next() {
            let key = entry.key().clone();
            drop(entry);
            budgets.remove(&key);
        }
    }
}

impl<S> Service<Request<Body>> for UserRateLimiterMiddleware<S>
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
            // Only apply per-user bandwidth budgets to authenticated requests.
            let is_authenticated = req.extensions().get::<AuthenticatedSession>().is_some();
            let pubky_host = req.extensions().get::<PubkyHost>().cloned();
            let user_limits = req.extensions().get::<UserLimitConfig>().cloned();

            if let (true, Some(pubky_host), Some(limits)) =
                (is_authenticated, pubky_host, user_limits)
            {
                let pubkey = pubky_host.public_key().clone();
                let is_write = is_write_method(req.method());

                let budget = if is_write {
                    limits.rate_write.as_ref()
                } else {
                    limits.rate_read.as_ref()
                };

                if let Some(budget) = budget {
                    // Ensure map entry exists
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

                    // Pre-check: is the budget already exhausted?
                    let dir = state.direction(is_write);
                    let (bytes_used, budget_bytes, seconds_remaining) =
                        dir.check_and_maybe_reset(budget);

                    if bytes_used >= budget_bytes {
                        let retry_after = seconds_remaining.max(1);
                        tracing::debug!(
                            "Per-user bandwidth budget exceeded for {} ({}, used={}B, budget={}B, retry_after={}s)",
                            pubkey.z32(),
                            if is_write { "write" } else { "read" },
                            bytes_used,
                            budget_bytes,
                            retry_after,
                        );
                        let mut response = HttpError::new_with_message(
                            StatusCode::TOO_MANY_REQUESTS,
                            "Per-user bandwidth budget exceeded",
                        )
                        .into_response();
                        response.headers_mut().insert(
                            axum::http::header::RETRY_AFTER,
                            axum::http::HeaderValue::from(retry_after),
                        );
                        return Ok(response);
                    }

                    if is_write {
                        // For writes, count bytes from Content-Length (always present
                        // for known-size bodies). For chunked/streaming uploads without
                        // Content-Length, wrap the body stream to count as chunks flow.
                        let content_length = req
                            .headers()
                            .get(axum::http::header::CONTENT_LENGTH)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok());

                        if let Some(total_bytes) = content_length {
                            // Known size: deduct immediately
                            dir.add_bytes(total_bytes);
                            let response = inner.call(req).await?;
                            Ok(response)
                        } else {
                            // Unknown size: wrap body stream to count bytes
                            let (parts, body) = req.into_parts();
                            let byte_counter = Arc::new(AtomicU64::new(0));
                            let counter_clone = byte_counter.clone();
                            let state_clone = state.clone();
                            let counted_stream = body.into_data_stream().map(move |chunk| {
                                if let Ok(ref bytes) = chunk {
                                    let len = bytes.len() as u64;
                                    counter_clone.fetch_add(len, Ordering::Relaxed);
                                    state_clone.write.add_bytes(len);
                                }
                                chunk
                            });
                            let new_body = Body::from_stream(counted_stream);
                            let new_req = Request::from_parts(parts, new_body);
                            let response = inner.call(new_req).await?;
                            Ok(response)
                        }
                    } else {
                        // For reads, wrap the response body to count bytes as they stream
                        let response = inner.call(req).await?;

                        let (parts, body) = response.into_parts();
                        let state_clone = state.clone();
                        let counted_stream = body.into_data_stream().map(move |chunk| {
                            if let Ok(ref bytes) = chunk {
                                let len = bytes.len() as u64;
                                state_clone.read.add_bytes(len);
                            }
                            chunk
                        });
                        let new_body = Body::from_stream(counted_stream);

                        Ok(axum::response::Response::from_parts(parts, new_body))
                    }
                } else {
                    inner.call(req).await
                }
            } else {
                inner.call(req).await
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
            .route("/test", get(ok_handler).post(ok_handler))
            .layer(UserRateLimiterLayer::new())
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

        // Write should succeed independently of read state
        let resp = app
            .clone()
            .oneshot(make_request(Method::POST, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
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
            let resp = app.clone().oneshot(make_unauthed(Method::POST)).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }
}
