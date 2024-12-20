use pkarr::PublicKey;

use crate::core::extractors::PubkyHost;

use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::core::error::Result;

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
        self.inner.poll_ready(cx).map_err(|_| unreachable!()) // `Infallible` conversion
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let mut req = req;

        Box::pin(async move {
            let headers_to_check = ["host", "pubky-host"];

            for header in headers_to_check {
                if let Some(Ok(pubky_host)) = req.headers().get(header).map(|h| h.to_str()) {
                    if let Ok(public_key) = PublicKey::try_from(pubky_host) {
                        req.extensions_mut().insert(PubkyHost(public_key));
                    }
                }
            }

            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}
