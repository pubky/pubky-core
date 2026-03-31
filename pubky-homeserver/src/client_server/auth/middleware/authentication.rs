//! Authentication middleware.
//!
//! The [`AuthenticationLayer`] tries to authenticate each request via Bearer JWT
//! or deprecated session cookie. On success it inserts an [`AuthSession`] into
//! request extensions.
//!
//! - **Bearer token present but invalid** → rejects with 401 and a specific error message.
//! - **No credentials or invalid cookie** → forwards without an identity (never rejects).

use crate::client_server::auth::cookie::auth::authenticate_cookie;
use crate::client_server::auth::jwt::auth::extract_bearer_token;
use crate::client_server::auth::jwt::auth::authenticate_bearer;
use crate::client_server::auth::jwt::crypto::jws_crypto::JwsCompact;
use crate::client_server::auth::AuthSession;
use crate::client_server::auth::AuthState;
use crate::client_server::middleware::pubky_host::PubkyHost;
use crate::shared::HttpError;
use axum::response::IntoResponse;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use pubky_common::crypto::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

// ── Layer ───────────────────────────────────────────────────────────────────

/// Tower layer that resolves credentials into an [`AuthSession`].
///
/// Inserts an `AuthSession` into request extensions when authentication
/// succeeds. Rejects with 401 if a Bearer token is present but invalid.
/// Requests without credentials or with invalid cookies are forwarded
/// without an identity for downstream layers to handle.
#[derive(Debug, Clone)]
pub struct AuthenticationLayer {
    state: AuthState,
}

impl AuthenticationLayer {
    pub fn new(state: AuthState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for AuthenticationLayer {
    type Service = AuthenticationMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthenticationMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

/// Middleware that resolves Bearer JWT or deprecated cookie credentials.
#[derive(Debug, Clone)]
pub struct AuthenticationMiddleware<S> {
    inner: S,
    state: AuthState,
}

impl<S> Service<Request<Body>> for AuthenticationMiddleware<S>
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
            let bearer_token = match extract_bearer_token(&req) {
                Ok(token) => token,
                Err(e) => return Ok(e.into_response()),
            };
            let cookies = req.extensions().get::<Cookies>().cloned();
            let pubky = req.extensions().get::<PubkyHost>().cloned();

            match resolve_auth_session(
                &state,
                bearer_token.as_ref(),
                cookies.as_ref(),
                pubky.as_ref().map(|p| p.public_key()),
            )
            .await
            {
                Ok(Some(session)) => {
                    req.extensions_mut().insert(session);
                }
                Ok(None) => {}
                Err(e) => return Ok(e.into_response()),
            }

            inner.call(req).await.map_err(|e| match e {})
        })
    }
}

/// Try to resolve an [`AuthSession`] from Bearer token or cookie.
///
/// - `Ok(Some(session))` — authentication succeeded.
/// - `Ok(None)` — no credentials presented (or cookie auth failed silently).
/// - `Err(HttpError)` — Bearer token was present but invalid.
async fn resolve_auth_session(
    state: &AuthState,
    bearer_token: Option<&JwsCompact>,
    cookies: Option<&Cookies>,
    public_key: Option<&PublicKey>,
) -> Result<Option<AuthSession>, HttpError> {
    if let Some(token) = bearer_token {
        return authenticate_bearer(state, token).await.map(Some);
    }

    let Some(cookies) = cookies else {
        return Ok(None);
    };
    let Some(public_key) = public_key else {
        return Ok(None);
    };
    Ok(authenticate_cookie(state, cookies, public_key).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_context::AppContext;
    use crate::client_server::auth::jwt::crypto::access_jwt_issuer::AccessJwt;
    use crate::client_server::auth::AuthState;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use pubky_common::auth::access_jwt::AccessJwtClaims;
    use pubky_common::auth::jws::{GrantId, TokenId};
    use pubky_common::auth::AuthVerifier;
    use pubky_common::crypto::Keypair;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state() -> (AuthState, Keypair) {
        let context = AppContext::test().await;
        let keypair = context.keypair.clone();
        let state = AuthState {
            auth_service: crate::client_server::auth::AuthService::new(
                context.sql_db.clone(),
                context.keypair.clone(),
            ),
            sql_db: context.sql_db.clone(),
            verifier: AuthVerifier::default(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
        };
        (state, keypair)
    }

    /// Inner service that asserts whether AuthSession was inserted into extensions.
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

    fn mint_jwt(homeserver_keypair: &Keypair) -> String {
        let user_kp = Keypair::random();
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = AccessJwtClaims {
            iss: homeserver_keypair.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: now,
            exp: now + 3600,
        };
        AccessJwt::mint(homeserver_keypair, &claims)
    }

    // --- middleware: no credentials ---

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn no_credentials_forwards_without_auth_session() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // --- middleware: Bearer token edge cases ---

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn malformed_bearer_token_rejects_with_401() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", "Bearer not-a-valid-jws")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn valid_jws_with_wrong_signature_rejects_with_401() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        let wrong_keypair = Keypair::random();
        let token = mint_jwt(&wrong_keypair);

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn valid_jwt_but_no_session_in_db_rejects_with_401() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state.clone()).layer(assert_handler(false));

        let token = mint_jwt(&_hs_keypair);

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn expired_jwt_rejects_with_401() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state.clone()).layer(assert_handler(false));

        let user_kp = Keypair::random();
        let claims = AccessJwtClaims {
            iss: _hs_keypair.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: 1000,
            exp: 2000, // far in the past
        };
        let token = AccessJwt::mint(&_hs_keypair, &claims);

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- middleware: non-Bearer auth schemes ---

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn basic_auth_header_rejected_with_401() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // --- middleware: cookie edge cases ---

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn cookie_with_no_pubky_host_forwards_without_auth() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

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
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

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

    // --- middleware: Bearer priority ---

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn invalid_bearer_rejects_even_with_valid_cookie_present() {
        let (state, _hs_keypair) = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        let pk = Keypair::random().public_key();
        let mut req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", "Bearer not-a-valid-jws")
            .header("Cookie", format!("{}=fakesecret", pk.z32()))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pk));

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
