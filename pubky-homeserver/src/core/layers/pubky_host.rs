use crate::core::error::Result;
use crate::core::extractors::PubkyHost;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

/// A Tower Layer to handle authorization for write operations.
#[derive(Debug, Clone)]
pub struct PubkyHostLayer;

impl<S> Layer<S> for PubkyHostLayer {
    type Service = PubkyHostLayerMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PubkyHostLayerMiddleware { inner }
    }
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct PubkyHostLayerMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for PubkyHostLayerMiddleware<S>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
        + Send
        + 'static
        + Clone,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|_| unreachable!())
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        // Use helper function to extract public key from headers or query parameter.
        if let Some(public_key) = extract_pubky(&req) {
            req.extensions_mut().insert(PubkyHost(public_key));
        }

        Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) })
    }
}

/// Helper function to extract the public key from request headers or query parameter.
fn extract_pubky(req: &Request<Body>) -> Option<PublicKey> {
    // Check headers "host" and "pubky-host"
    for header in ["host", "pubky-host"] {
        if let Some(value) = req.headers().get(header) {
            if let Ok(s) = value.to_str() {
                if let Ok(pubky) = PublicKey::try_from(s) {
                    return Some(pubky);
                }
            }
        }
    }

    // Fallback: check query string for "pubky-host"
    req.uri().query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                if key == "pubky-host" {
                    return PublicKey::try_from(value).ok();
                }
            }
            None
        })
    })
}
