// src/core/layers/admin_auth.rs
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

/// A Tower Layer that checks the “X-Admin-Password” header against a configured password.
#[derive(Clone)]
pub struct AdminAuthLayer {
    password: String,
}

impl AdminAuthLayer {
    /// Create a new AdminAuthLayer with the given admin password.
    pub fn new(password: String) -> Self {
        Self { password }
    }
}

impl<S> Layer<S> for AdminAuthLayer {
    type Service = AdminAuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AdminAuthMiddleware {
            inner,
            password: self.password.clone(),
        }
    }
}

/// Middleware that performs the admin password check.
#[derive(Clone)]
pub struct AdminAuthMiddleware<S> {
    inner: S,
    password: String,
}

impl<S, ReqBody> Service<Request<ReqBody>> for AdminAuthMiddleware<S>
where
    S: Service<Request<ReqBody>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let password = self.password.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            match req.headers().get("X-Admin-Password") {
                Some(header_value) if header_value.to_str().unwrap_or("") == password => {
                    // If the header is valid, proceed.
                    inner.call(req).await
                }
                Some(_) => {
                    // If header exists but password is incorrect,
                    let msg = "Invalid admin password";
                    let response = Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::from(msg))
                        .unwrap_or_else(|_| Response::new(Body::from(msg)));
                    Ok(response)
                }
                None => {
                    // If header is missing, do the same.
                    let msg = "Missing admin password";
                    let response = Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::from(msg))
                        .unwrap_or_else(|_| Response::new(Body::from(msg)));
                    Ok(response)
                }
            }
        })
    }
}
