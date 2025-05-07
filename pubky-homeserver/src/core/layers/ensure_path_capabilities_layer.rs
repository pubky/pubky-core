use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::core::error::{Error, Result};
use crate::core::sessions::UserSession;

/// A Tower Layer that ensures the user has write access to the path.
/// 
/// Requires the `SessionRequiredLayer` to be applied first.
/// 
/// Used by the `write` routes.
///
#[derive(Debug, Clone)]
pub struct EnsurePathCapabilitiesLayer {}

impl EnsurePathCapabilitiesLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> Layer<S> for EnsurePathCapabilitiesLayer {
    type Service = Middleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Middleware { inner }
    }
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct Middleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for Middleware<S>
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

        let path = req.uri().path().to_string();

        Box::pin(async move {
            let UserSession { session, .. } = match req.extensions().get::<UserSession>() {
                Some(session) => session,
                None => {
                    return Ok(Error::new(StatusCode::INTERNAL_SERVER_ERROR, Some("SessionRequiredLayer is not available. Did you forget to apply it?".to_string())).into_response());
                }
            };

            let is_authorized = session.capabilities().iter().any(|cap| {
                path.starts_with(&cap.scope)
                    && cap
                        .actions
                        .contains(&pubky_common::capabilities::Action::Write)
            });

            if !is_authorized {
                return Ok(Error::with_status(StatusCode::FORBIDDEN).into_response());
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}
