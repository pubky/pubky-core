//! Bearer token authentication middleware.
//!
//! The [`GrantAuthenticationLayer`] extracts the opaque Bearer token from the
//! `Authorization` header and asks the `AuthService` to resolve it into a
//! `GrantSession`. On success it inserts an [`AuthSession`] into request extensions.
//! The middleware never rejects — downstream handlers declare their auth
//! requirement via the extractor type (`AuthSession` for strict, `Option<AuthSession>`
//! for lenient).
//!
//! - **Bearer token present and valid** → inserts `AuthSession::Grant`.
//! - **Bearer token present but unknown/expired/revoked** → forwards without an
//!   identity; the downstream extractor emits 401 if the route requires auth.
//! - **No Authorization header** → forwards without an identity.
//! - **Non-Bearer / malformed Authorization header** → forwards without an identity.

use crate::client_server::auth::grant::crypto::session_token::SessionBearer;
use crate::client_server::auth::{AuthSession, AuthState};
use axum::http::header;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

// ── Bearer token extraction ────────────────────────────────────────────────

enum BearerTokenExtraction {
    Present(SessionBearer),
    Missing,
    Invalid,
}

fn extract_bearer_token(req: &Request<Body>) -> BearerTokenExtraction {
    let Some(value) = req.headers().get(header::AUTHORIZATION) else {
        return BearerTokenExtraction::Missing;
    };
    let Ok(value) = value.to_str() else {
        return BearerTokenExtraction::Invalid;
    };

    let Some(raw_token) = value.strip_prefix("Bearer ") else {
        return BearerTokenExtraction::Invalid;
    };

    match SessionBearer::parse(raw_token) {
        Ok(bearer) => BearerTokenExtraction::Present(bearer),
        Err(_) => BearerTokenExtraction::Invalid,
    }
}

// ── Layer ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GrantAuthenticationLayer {
    state: AuthState,
}

impl GrantAuthenticationLayer {
    pub fn new(state: AuthState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for GrantAuthenticationLayer {
    type Service = GrantAuthenticationMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrantAuthenticationMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

// ── Middleware ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GrantAuthenticationMiddleware<S> {
    inner: S,
    state: AuthState,
}

impl<S> Service<Request<Body>> for GrantAuthenticationMiddleware<S>
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
        self.inner.poll_ready(cx).map_err(|e| match e {})
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let state = self.state.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let bearer = match extract_bearer_token(&req) {
                BearerTokenExtraction::Present(bearer) => bearer,
                BearerTokenExtraction::Missing => {
                    return inner.call(req).await.map_err(|e| match e {});
                }
                BearerTokenExtraction::Invalid => {
                    tracing::debug!(
                        "Authorization header present but not a usable Bearer token; forwarding without auth"
                    );
                    return inner.call(req).await;
                }
            };

            match state
                .grant_auth_service
                .resolve_grant_session_by_bearer(&bearer)
                .await
            {
                Ok(session) => {
                    req.extensions_mut().insert(AuthSession::Grant(session));
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Bearer token did not resolve to a grant session; forwarding without auth"
                    );
                }
            }

            inner.call(req).await.map_err(|e| match e {})
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_context::AppContext;
    use crate::client_server::auth::AuthSession;
    use crate::client_server::auth::AuthState;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use pubky_common::crypto::Keypair;
    use std::sync::Arc;
    use tower::ServiceExt;

    /// A 43-char stand-in for a well-formed but unknown bearer (same shape as
    /// what the server mints but never issued → service-layer miss).
    const UNKNOWN_WELL_FORMED_BEARER: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";

    /// Any length other than 43 — test wants to probe the parse-layer reject.
    const WRONG_LENGTH_BEARER_LEN: usize = 200;

    async fn test_state() -> (AuthState, Keypair) {
        let context = AppContext::test().await;
        let keypair = context.keypair.clone();
        (AuthState::new(&context), keypair)
    }

    fn assert_handler(
        expect_auth: bool,
    ) -> impl Service<
        Request<Body>,
        Response = axum::response::Response,
        Error = Infallible,
        Future = impl Send,
    > + Clone {
        let expect_auth = Arc::new(expect_auth);
        tower::service_fn(move |req: Request<Body>| {
            let expect_auth = expect_auth.clone();
            async move {
                let has_auth = req.extensions().get::<AuthSession>().is_some();
                assert_eq!(
                    has_auth, *expect_auth,
                    "AuthSession presence mismatch: expected={}, actual={}",
                    *expect_auth, has_auth
                );
                Ok::<_, Infallible>(StatusCode::OK.into_response())
            }
        })
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn no_auth_header_forwards_without_session() {
        let (state, _) = test_state().await;
        let svc = GrantAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn unknown_bearer_forwards_without_session() {
        let (state, _) = test_state().await;
        let svc = GrantAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header(
                "Authorization",
                format!("Bearer {UNKNOWN_WELL_FORMED_BEARER}"),
            )
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn wrong_length_bearer_forwards_without_session() {
        let (state, _) = test_state().await;
        let svc = GrantAuthenticationLayer::new(state).layer(assert_handler(false));

        let huge = "a".repeat(WRONG_LENGTH_BEARER_LEN);
        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", format!("Bearer {huge}"))
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn basic_auth_header_forwards_without_session() {
        let (state, _) = test_state().await;
        let svc = GrantAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── extract_bearer_token unit tests ────────────────────────────────

    #[test]
    fn extract_bearer_no_auth_header() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert!(matches!(
            extract_bearer_token(&req),
            BearerTokenExtraction::Missing
        ));
    }

    #[test]
    fn extract_bearer_basic_auth_rejected() {
        let req = Request::builder()
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        assert!(matches!(
            extract_bearer_token(&req),
            BearerTokenExtraction::Invalid
        ));
    }

    #[test]
    fn extract_bearer_empty_token() {
        let req = Request::builder()
            .header("Authorization", "Bearer ")
            .body(Body::empty())
            .unwrap();
        assert!(matches!(
            extract_bearer_token(&req),
            BearerTokenExtraction::Invalid
        ));
    }

    #[test]
    fn extract_bearer_wrong_length_token() {
        let huge = "a".repeat(WRONG_LENGTH_BEARER_LEN);
        let req = Request::builder()
            .header("Authorization", format!("Bearer {huge}"))
            .body(Body::empty())
            .unwrap();
        assert!(matches!(
            extract_bearer_token(&req),
            BearerTokenExtraction::Invalid
        ));
    }

    #[test]
    fn extract_bearer_valid_token() {
        let req = Request::builder()
            .header(
                "Authorization",
                format!("Bearer {UNKNOWN_WELL_FORMED_BEARER}"),
            )
            .body(Body::empty())
            .unwrap();
        let BearerTokenExtraction::Present(bearer) = extract_bearer_token(&req) else {
            panic!("expected bearer token to be present");
        };
        assert_eq!(bearer.as_str(), UNKNOWN_WELL_FORMED_BEARER);
    }
}
