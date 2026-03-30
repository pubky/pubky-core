//! Per-user rate limiting middleware.
//!
//! Reads the resolved `UserLimitConfig` from request extensions (set by `UserLimitResolverLayer`)
//! and enforces per-user read/write rate limits using governor.
//! Separate from the global path-based `RateLimiterLayer`.

use std::convert::Infallible;
use std::sync::{Arc, Weak};
use std::task::Poll;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::IntoResponse;
use dashmap::DashMap;
use futures_util::future::BoxFuture;
use governor::clock::{Clock, QuantaClock};
use governor::state::keyed::DashMapStateStore;
use governor::RateLimiter;
use pubky_common::crypto::PublicKey;
use tower::{Layer, Service};

use crate::client_server::extractors::PubkyHost;
use crate::client_server::layers::authz::AuthenticatedSession;
use crate::data_directory::quota_config::QuotaValue;
use crate::data_directory::user_limit_config::UserLimitConfig;
use crate::shared::HttpError;

type KeyedRateLimiter = RateLimiter<PublicKey, DashMapStateStore<PublicKey>, QuantaClock>;

/// A rate limiter paired with the quota it was created from,
/// so we can detect when the quota has changed and recreate.
#[derive(Debug)]
struct TrackedLimiter {
    quota: QuotaValue,
    limiter: Arc<KeyedRateLimiter>,
}

/// Per-user governor instances for read and write.
#[derive(Debug, Default)]
struct PerUserLimiters {
    read: Option<TrackedLimiter>,
    write: Option<TrackedLimiter>,
}

impl PerUserLimiters {
    /// Run `retain_recent` on all governor stores and return whether any state is still active.
    fn retain_recent_and_is_active(&self) -> bool {
        if let Some(ref tracked) = self.read {
            tracked.limiter.retain_recent();
        }
        if let Some(ref tracked) = self.write {
            tracked.limiter.retain_recent();
        }
        let read_active = self
            .read
            .as_ref()
            .is_some_and(|t| !t.limiter.is_empty());
        let write_active = self
            .write
            .as_ref()
            .is_some_and(|t| !t.limiter.is_empty());
        read_active || write_active
    }
}

/// Maximum number of distinct users tracked in the per-user rate limiter map.
/// When the map reaches this capacity and a new user needs to be inserted,
/// stale entries are evicted to make room. This prevents memory exhaustion
/// from an attacker rotating public keys while always enforcing rate limits.
const MAX_TRACKED_USERS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct UserRateLimiterLayer {
    limiters: Arc<DashMap<PublicKey, PerUserLimiters>>,
}

impl UserRateLimiterLayer {
    pub fn new() -> Self {
        let limiters: Arc<DashMap<PublicKey, PerUserLimiters>> = Arc::new(DashMap::new());

        // Periodic cleanup: remove entries whose governor stores are empty after retain_recent.
        // Uses a Weak reference so the task exits when the middleware is dropped.
        let limiters_weak: Weak<DashMap<PublicKey, PerUserLimiters>> = Arc::downgrade(&limiters);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                let Some(limiters_ref) = limiters_weak.upgrade() else {
                    break;
                };
                limiters_ref.retain(|_, v| v.retain_recent_and_is_active());
            }
        });

        Self { limiters }
    }
}

impl<S> Layer<S> for UserRateLimiterLayer {
    type Service = UserRateLimiterMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        UserRateLimiterMiddleware {
            inner,
            limiters: self.limiters.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRateLimiterMiddleware<S> {
    inner: S,
    limiters: Arc<DashMap<PublicKey, PerUserLimiters>>,
}

/// Returns true if the HTTP method is a "write" operation.
fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Evict stale entries from the limiter map when it reaches capacity.
/// First runs `retain_recent` on all governor stores and removes entries with no
/// active rate state. If the map is still at capacity after that, removes an
/// arbitrary entry to guarantee space for the new user.
fn evict_stale_entries(limiters: &DashMap<PublicKey, PerUserLimiters>) {
    limiters.retain(|_, v| v.retain_recent_and_is_active());

    // If retain didn't free enough space, forcibly remove an arbitrary entry.
    if limiters.len() >= MAX_TRACKED_USERS {
        if let Some(entry) = limiters.iter().next() {
            let key = entry.key().clone();
            drop(entry);
            limiters.remove(&key);
        }
    }
}

/// Get or recreate a limiter for the given slot, handling quota changes.
fn get_or_update_limiter(
    slot: &mut Option<TrackedLimiter>,
    quota_val: &QuotaValue,
) -> Arc<KeyedRateLimiter> {
    let needs_recreate = !matches!(slot, Some(ref tracked) if tracked.quota == *quota_val);
    if needs_recreate {
        let quota: governor::Quota = quota_val.clone().into();
        let limiter = Arc::new(RateLimiter::keyed(quota));
        *slot = Some(TrackedLimiter {
            quota: quota_val.clone(),
            limiter: limiter.clone(),
        });
        limiter
    } else {
        slot.as_ref().unwrap().limiter.clone()
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
        let limiters = self.limiters.clone();

        Box::pin(async move {
            // Only apply per-user rate limits to authenticated requests.
            // Anonymous requests are handled by the global IP/path rate limiter.
            let is_authenticated = req.extensions().get::<AuthenticatedSession>().is_some();
            let pubky_host = req.extensions().get::<PubkyHost>().cloned();
            let user_limits = req.extensions().get::<UserLimitConfig>().cloned();

            if let (true, Some(pubky_host), Some(limits)) =
                (is_authenticated, pubky_host, user_limits)
            {
                let pubkey = pubky_host.public_key().clone();
                let is_write = is_write_method(req.method());

                // Parse the rate string into a QuotaValue
                let quota_value = if is_write {
                    limits.parsed_rate_write()
                } else {
                    limits.parsed_rate_read()
                };

                if let Some(quota_val) = &quota_value {
                    // If the map is at capacity and this is a new user, evict stale
                    // entries to make room. This ensures rate limits are always enforced.
                    if !limiters.contains_key(&pubkey) && limiters.len() >= MAX_TRACKED_USERS {
                        evict_stale_entries(&limiters);
                    }

                    let mut entry = limiters
                        .entry(pubkey.clone())
                        .or_default();

                    let limiter = if is_write {
                        get_or_update_limiter(&mut entry.write, quota_val)
                    } else {
                        get_or_update_limiter(&mut entry.read, quota_val)
                    };
                    drop(entry);

                    if let Err(not_until) = limiter.check_key(&pubkey) {
                        let retry_after_secs = not_until
                            .wait_time_from(QuantaClock::default().now())
                            .as_secs()
                            .saturating_add(1); // round up to next whole second
                        tracing::debug!(
                            "Per-user rate limit exceeded for {} ({}, retry_after={}s)",
                            pubkey.z32(),
                            if is_write { "write" } else { "read" },
                            retry_after_secs,
                        );
                        let mut response = HttpError::new_with_message(
                            StatusCode::TOO_MANY_REQUESTS,
                            "Per-user rate limit exceeded",
                        )
                        .into_response();
                        response.headers_mut().insert(
                            axum::http::header::RETRY_AFTER,
                            axum::http::HeaderValue::from(retry_after_secs),
                        );
                        return Ok(response);
                    }
                }
            }

            inner.call(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;
    use pubky_common::crypto::Keypair;
    use tower::ServiceExt;

    use crate::client_server::extractors::PubkyHost;
    use crate::client_server::layers::authz::AuthenticatedSession;
    use crate::data_directory::user_limit_config::UserLimitConfig;

    use super::*;

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

    #[tokio::test]
    async fn test_rate_limit_read_enforced() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: Some("1r/m".to_string()),
            ..Default::default()
        };

        // First request succeeds (consumes the 1 allowed request)
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second request within the same minute should be rejected
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_rate_limit_write_enforced() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_write: Some("1r/m".to_string()),
            ..Default::default()
        };

        // First write succeeds
        let resp = app
            .clone()
            .oneshot(make_request(Method::POST, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second write should be rejected
        let resp = app
            .clone()
            .oneshot(make_request(Method::POST, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_no_pubky_host_passes_through() {
        let app = test_app();
        // Request without PubkyHost extension — should pass through without rate limiting
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
        // Limits with no rate_read/rate_write — unlimited
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
    async fn test_read_and_write_limits_independent() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: Some("1r/m".to_string()),
            rate_write: Some("1r/m".to_string()),
            ..Default::default()
        };

        // Exhaust read limit
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // Write should still be allowed (separate limiter)
        let resp = app
            .clone()
            .oneshot(make_request(Method::POST, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_different_users_have_separate_limits() {
        let app = test_app();
        let pubkey1 = Keypair::random().public_key();
        let pubkey2 = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: Some("1r/m".to_string()),
            ..Default::default()
        };

        // Exhaust user1's limit
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey1, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey1, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // User2 should still be allowed
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey2, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_retry_after_header_present() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: Some("1r/m".to_string()),
            ..Default::default()
        };

        // Exhaust the limit
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Second request should return 429 with Retry-After header
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &limits))
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
    async fn test_quota_change_recreates_limiter() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();

        // Start with a tight limit of 1r/m
        let tight_limits = UserLimitConfig {
            rate_read: Some("1r/m".to_string()),
            ..Default::default()
        };

        // Exhaust the tight limit
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &tight_limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &tight_limits))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

        // Now the admin loosens the limit to 100r/m — the limiter should be
        // recreated with a fresh bucket, allowing requests again.
        let loose_limits = UserLimitConfig {
            rate_read: Some("100r/m".to_string()),
            ..Default::default()
        };
        let resp = app
            .clone()
            .oneshot(make_request(Method::GET, &pubkey, &loose_limits))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Quota change should recreate the limiter, allowing new requests"
        );
    }

    #[tokio::test]
    async fn test_unauthenticated_request_bypasses_rate_limit() {
        let app = test_app();
        let pubkey = Keypair::random().public_key();
        let limits = UserLimitConfig {
            rate_read: Some("1r/m".to_string()),
            ..Default::default()
        };

        // Build requests WITHOUT AuthenticatedSession marker
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
            let resp = app.clone().oneshot(make_unauthed(Method::GET)).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }
}
