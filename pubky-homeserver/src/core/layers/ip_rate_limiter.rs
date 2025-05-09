//!
//! Implements rate limiting with governor.
//!
//! Would love to use tower_governor but I can't type it properly due to
//! https://github.com/benwis/tower-governor/issues/49.
//!
//! So we implement our own rate limiter here.
//! Maybe one day we can switch to tower_governor if the issue is fixed.
//!

use axum::extract::FromRequest;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use governor::clock::QuantaClock;
use governor::state::keyed::DashMapStateStore;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::core::error::Result;
use crate::core::Error;
use crate::quota_config::QuotaConfig;
use governor::{Quota, RateLimiter};

/// A Tower Layer to handle ip rate limiting.
#[derive(Debug, Clone)]
pub struct IpRateLimiterLayer {
    config: Option<QuotaConfig>,
}

impl IpRateLimiterLayer {
    /// Create a new rate limiter layer with the given quota.
    ///
    /// Example quota:
    /// ```
    /// let quota = Quota::per_minute(1.try_into().unwrap()).allow_burst(1.try_into().unwrap());
    /// ```
    pub fn new(quota: Option<QuotaConfig>) -> Self {
        Self { config: quota }
    }
}

impl<S> Layer<S> for IpRateLimiterLayer {
    type Service = IpRateLimiterMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let limiter = self.config.clone().map(|c| {
            Arc::new(RateLimiter::keyed(c.quota))
        });
        IpRateLimiterMiddleware {
            inner,
            config: self.config.clone(),
            limiter,
        }
    }
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct IpRateLimiterMiddleware<S> {
    inner: S,
    config: Option<QuotaConfig>,
    limiter: Option<Arc<RateLimiter<IpAddr, DashMapStateStore<IpAddr>, QuantaClock>>>,
}

impl<S> IpRateLimiterMiddleware<S> {
    fn is_enabled(&self) -> bool {
        self.config.is_some()
    }

    fn extract_key(&self, req: &Request<Body>) -> Option<IpAddr> {
        let headers = req.headers();
        maybe_x_forwarded_for(headers)
            .or_else(|| maybe_x_real_ip(headers))
            .or_else(|| maybe_connect_info(&req))
    }
}

impl<S> Service<Request<Body>> for IpRateLimiterMiddleware<S>
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
        self.inner.poll_ready(cx).map_err(|_| unreachable!()) // `Infallible` conversion
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        if !self.is_enabled() {
            return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
        }

        let limiter = self.limiter.as_ref().expect("limiter is always set");

        let key = match self.extract_key(&req) {
            Some(key) => key,
            None => {
                tracing::warn!("Failed to extract ip from header for rate limiting.");
                return Box::pin(async move {
                    Ok(Error::new(StatusCode::INTERNAL_SERVER_ERROR, Some("failed to extract ip for rate limiting.".to_string())).into_response())
                });
            }
        };
        

        match limiter.check_key(&key) {
            Ok(()) => {
                // Rate limit not exceeded, call the next layer
                return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
            }
            Err(e) => {
                tracing::warn!("Rate limit exceeded for {key}: {}", e);
                return Box::pin(async move {
                    Ok(Error::new(
                        StatusCode::TOO_MANY_REQUESTS,
                        Some("rate limit exceeded".to_string()),
                    )
                    .into_response())
                });
            }
        };
    }
}

// From https://github.com/benwis/tower-governor/blob/main/src/key_extractor.rs#L121

const X_REAL_IP: &str = "x-real-ip";
const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Tries to parse the `x-forwarded-for` header
fn maybe_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_FORWARDED_FOR)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.split(',').find_map(|s| s.trim().parse::<IpAddr>().ok()))
}

/// Tries to parse the `x-real-ip` header
fn maybe_x_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_REAL_IP)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.parse::<IpAddr>().ok())
}

fn maybe_connect_info<T>(req: &Request<T>) -> Option<IpAddr> {
    req.extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|addr| addr.ip())
}


struct ThrottledBody(Body);

#[async_trait]
impl<S> FromRequest<S> for ThrottledBody
where
    Body: Send + 'static,
    S: Send + Sync,
{
    type Rejection = ();

    async fn from_request(req: Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let body = req.into_body();
        let throttled = body
            .map(|chunk| async {
                tokio::time::sleep(Duration::from_millis(10)).await; // slow down
                chunk
            })
            .buffered(1);

        let new_body = Body::wrap_stream(throttled);
        Ok(ThrottledBody(new_body))
    }
}