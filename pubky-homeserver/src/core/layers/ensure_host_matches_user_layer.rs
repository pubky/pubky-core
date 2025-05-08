use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::core::sessions::UserSession;
use crate::core::{
    error::{Error, Result},
    extractors::PubkyHost,
};

/// A Tower Layer that compares the pubky host with the session's pubky.
///
/// If the host does not match, it returns a 403 Forbidden error.
///
/// This ensures that a user cannot write to a different pubky than the one they are logged in with.
///
#[derive(Debug, Clone)]
pub struct EnsureHostMatchesUserLayer {}

impl EnsureHostMatchesUserLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> Layer<S> for EnsureHostMatchesUserLayer {
    type Service = EnsureHostMatchesUserMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        EnsureHostMatchesUserMiddleware { inner }
    }
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct EnsureHostMatchesUserMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for EnsureHostMatchesUserMiddleware<S>
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

        Box::pin(async move {
            let host = match req.extensions().get::<PubkyHost>() {
                Some(pk) => pk,
                None => {
                    return Ok(
                        Error::new(StatusCode::NOT_FOUND, "Pubky Host is missing".into())
                            .into_response(),
                    )
                }
            };

            let session = match req.extensions().get::<UserSession>() {
                Some(session) => session,
                None => {
                    return Ok(Error::with_status(StatusCode::UNAUTHORIZED).into_response());
                }
            };

            if session.session.pubky() != host.public_key() {
                return Ok(Error::with_status(StatusCode::FORBIDDEN).into_response());
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}
