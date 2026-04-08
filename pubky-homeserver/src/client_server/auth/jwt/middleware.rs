//! JWT Bearer token authentication middleware.
//!
//! The [`JwtAuthenticationLayer`] extracts and validates Bearer tokens from the
//! `Authorization` header. On success it inserts an [`AuthSession`] into
//! request extensions.
//!
//! - **Bearer token present and valid** → inserts `AuthSession::Grant`.
//! - **Bearer token present but invalid** → rejects with 401.
//! - **No Authorization header** → forwards without an identity (never rejects).
//! - **Non-Bearer Authorization scheme** → rejects with 401.

use crate::client_server::auth::jwt::crypto::access_jwt_issuer::verify_access_jwt;
use crate::client_server::auth::jwt::crypto::jws_crypto::JwsCompact;
use crate::client_server::auth::{AuthSession, AuthState};
use crate::shared::HttpError;
use axum::http::header;
use axum::response::IntoResponse;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

// ── Bearer token extraction ────────────────────────────────────────────────

/// Extract and parse Bearer token from the Authorization header.
///
/// - `Ok(Some(token))` — valid Bearer token found.
/// - `Ok(None)` — no Authorization header present.
/// - `Err(HttpError)` — Authorization header present but not a valid Bearer token.
fn extract_bearer_token(req: &Request<Body>) -> Result<Option<JwsCompact>, HttpError> {
    let Some(value) = req.headers().get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| HttpError::unauthorized_with_message("Malformed Authorization header"))?;

    let Some(raw_token) = value.strip_prefix("Bearer ") else {
        return Err(HttpError::unauthorized_with_message("Malformed Authorization header"));
    };
    let token = JwsCompact::parse(raw_token)
        .map_err(|_| HttpError::unauthorized_with_message("Malformed Bearer token"))?;
    Ok(Some(token))
}

// ── Layer ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct JwtAuthenticationLayer {
    state: AuthState,
}

impl JwtAuthenticationLayer {
    pub fn new(state: AuthState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for JwtAuthenticationLayer {
    type Service = JwtAuthenticationMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        JwtAuthenticationMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

// ── Middleware ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct JwtAuthenticationMiddleware<S> {
    inner: S,
    state: AuthState,
}

impl<S> Service<Request<Body>> for JwtAuthenticationMiddleware<S>
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
            let token = match extract_bearer_token(&req) {
                Ok(Some(token)) => token,
                Ok(None) => return inner.call(req).await.map_err(|e| match e {}),
                Err(e) => return Ok(e.into_response()),
            };

            // Verify JWT signature/expiry, then resolve the session via AuthService.
            let jwt = match verify_access_jwt(&token, &state.auth_service.homeserver_public_key())
            {
                Ok(jwt) => jwt,
                Err(_) => {
                    return Ok(
                        HttpError::unauthorized_with_message("Invalid or expired JWT")
                            .into_response(),
                    )
                }
            };
            match state.auth_service.resolve_grant_session(&jwt).await {
                Ok(session) => {
                    req.extensions_mut().insert(AuthSession::Grant(session));
                }
                Err(e) => return Ok(HttpError::from(e).into_response()),
            }

            inner.call(req).await.map_err(|e| match e {})
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_context::AppContext;
    use crate::client_server::auth::jwt::crypto::access_jwt_issuer::mint_access_jwt;
    use crate::client_server::auth::AuthSession;
    use crate::client_server::auth::AuthState;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use pubky_common::auth::access_jwt::AccessJwtClaims;
    use pubky_common::auth::jws::{GrantId, TokenId};
    use crate::client_server::auth::cookie::verifier::AuthVerifier;
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
        mint_access_jwt(homeserver_keypair, &claims).to_string()
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn no_auth_header_forwards_without_session() {
        let (state, _) = test_state().await;
        let svc = JwtAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn malformed_bearer_token_rejects_with_401() {
        let (state, _) = test_state().await;
        let svc = JwtAuthenticationLayer::new(state).layer(assert_handler(false));

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
        let (state, _) = test_state().await;
        let svc = JwtAuthenticationLayer::new(state).layer(assert_handler(false));

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
        let (state, hs_keypair) = test_state().await;
        let svc = JwtAuthenticationLayer::new(state).layer(assert_handler(false));

        let token = mint_jwt(&hs_keypair);

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
        let (state, hs_keypair) = test_state().await;
        let svc = JwtAuthenticationLayer::new(state).layer(assert_handler(false));

        let user_kp = Keypair::random();
        let claims = AccessJwtClaims {
            iss: hs_keypair.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: 1000,
            exp: 2000, // far in the past
        };
        let token = mint_access_jwt(&hs_keypair, &claims);

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
    async fn basic_auth_header_rejected_with_401() {
        let (state, _) = test_state().await;
        let svc = JwtAuthenticationLayer::new(state).layer(assert_handler(false));

        let req = Request::builder()
            .uri("/pub/file.txt")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();

        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── extract_bearer_token unit tests ────────────────────────────────

    #[test]
    fn extract_bearer_no_auth_header() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert!(matches!(extract_bearer_token(&req), Ok(None)));
    }

    #[test]
    fn extract_bearer_basic_auth_rejected() {
        let req = Request::builder()
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        assert!(extract_bearer_token(&req).is_err());
    }

    #[test]
    fn extract_bearer_malformed_token() {
        let req = Request::builder()
            .header("Authorization", "Bearer not-a-jws")
            .body(Body::empty())
            .unwrap();
        assert!(extract_bearer_token(&req).is_err());
    }

    #[test]
    fn extract_bearer_empty_token() {
        let req = Request::builder()
            .header("Authorization", "Bearer ")
            .body(Body::empty())
            .unwrap();
        assert!(extract_bearer_token(&req).is_err());
    }

    #[test]
    fn extract_bearer_valid_jws_format() {
        let req = Request::builder()
            .header("Authorization", "Bearer aaa.bbb.ccc")
            .body(Body::empty())
            .unwrap();
        let result = extract_bearer_token(&req).unwrap();
        assert!(result.is_some());
    }
}
