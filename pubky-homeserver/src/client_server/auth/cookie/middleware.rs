//! Cookie-based authentication middleware.
//!
//! The [`CookieAuthenticationLayer`] tries to authenticate via deprecated
//! session cookies. It only activates when no [`AuthSession`] has been set
//! by a prior middleware (e.g. JWT).
//!
//! - **AuthSession already present** → skips (JWT took priority).
//! - **Valid cookie** → inserts `AuthSession::Cookie`.
//! - **No cookies or invalid cookie** → forwards without an identity (never rejects).

use crate::client_server::auth::cookie::auth::authenticate_cookie;
use crate::client_server::auth::AuthSession;
use crate::client_server::auth::AuthState;
use crate::client_server::middleware::pubky_host::PubkyHost;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

// ── Layer ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CookieAuthenticationLayer {
    state: AuthState,
}

impl CookieAuthenticationLayer {
    pub fn new(state: AuthState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for CookieAuthenticationLayer {
    type Service = CookieAuthenticationMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CookieAuthenticationMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

// ── Middleware ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CookieAuthenticationMiddleware<S> {
    inner: S,
    state: AuthState,
}

impl<S> Service<Request<Body>> for CookieAuthenticationMiddleware<S>
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
            // Skip if a prior middleware (JWT) already authenticated the request.
            if req.extensions().get::<AuthSession>().is_some() {
                return inner.call(req).await.map_err(|e| match e {});
            }

            let cookies = req.extensions().get::<Cookies>().cloned();
            let pubky = req.extensions().get::<PubkyHost>().cloned();

            if let (Some(cookies), Some(pubky)) = (cookies, pubky) {
                if let Some(session) =
                    authenticate_cookie(&state, &cookies, pubky.public_key()).await
                {
                    req.extensions_mut().insert(session);
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
    use crate::client_server::auth::cookie::verifier::AuthVerifier;
    use pubky_common::crypto::Keypair;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state() -> AuthState {
        let context = AppContext::test().await;
        AuthState {
            auth_service: crate::client_server::auth::AuthService::new(
                context.sql_db.clone(),
                context.keypair.clone(),
            ),
            sql_db: context.sql_db.clone(),
            verifier: AuthVerifier::default(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        }
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
    async fn no_cookies_forwards_without_auth() {
        let state = test_state().await;
        let svc = CookieAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn cookie_with_no_pubky_host_forwards_without_auth() {
        let state = test_state().await;
        let svc = CookieAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/session")
            .header("Cookie", "somekey=somevalue")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn cookie_with_unknown_session_secret_forwards_without_auth() {
        let state = test_state().await;
        let svc = CookieAuthenticationLayer::new(state).layer(assert_handler(false));

        let pk = Keypair::random().public_key();
        let mut req = Request::builder()
            .uri("/session")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pk.clone()));
        let cookies = tower_cookies::Cookies::default();
        cookies.add(tower_cookies::Cookie::new(
            pk.z32(),
            "nonexistent-secret-value",
        ));
        req.extensions_mut().insert(cookies);

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
