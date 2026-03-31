//! Authentication middleware and extractor.
//!
//! The [`AuthenticationLayer`] tries to authenticate each request via Bearer JWT
//! or deprecated session cookie. On success it inserts an [`AuthSession`] into
//! request extensions.
//!
//! - **Bearer token present but invalid** → rejects with 401 and a specific error message.
//! - **No credentials or invalid cookie** → forwards without an identity (never rejects).
//!
//! # Usage
//!
//! Apply as a shared layer on all routes that may need authentication:
//!
//! ```rust,ignore
//! use crate::client_server::middleware::authentication::AuthenticationLayer;
//!
//! let app = Router::new()
//!     .route("/session", get(session_handler))
//!     .route("/{*path}", get(read_handler).put(write_handler))
//!     .layer(AuthenticationLayer::new(state));
//! ```
//!
//! Handlers extract the resolved identity via [`AuthSession`]:
//!
//! ```rust,ignore
//! // Require authentication (returns 401 if absent):
//! async fn protected_handler(auth: AuthSession) -> impl IntoResponse {
//!     let pubkey = auth.user_key();
//!     let caps = auth.capabilities();
//!     // ...
//! }
//!
//! // Optional authentication (never rejects):
//! async fn public_handler(auth: Option<AuthSession>) -> impl IntoResponse {
//!     if let Some(auth) = auth {
//!         // authenticated request
//!     } else {
//!         // anonymous request
//!     }
//! }
//! ```

use crate::client_server::auth::crypto::access_jwt_issuer::AccessJwt;
use crate::client_server::auth::crypto::jws_crypto::JwsCompact;
use crate::client_server::auth::persistence::grant::{GrantEntity, GrantRepository};
use crate::client_server::auth::persistence::grant_session::GrantSessionRepository;
use crate::client_server::middleware::pubky_host::PubkyHost;
use crate::client_server::AppState;
use crate::persistence::sql::session::{SessionEntity, SessionRepository, SessionSecret};
use crate::shared::HttpError;
use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use pubky_common::auth::jws::{GrantId, TokenId};
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

// ── Extractor ───────────────────────────────────────────────────────────────

/// Get the session secret from the cookies.
/// Returns `None` if the session secret is not found or invalid.
pub fn session_secret_from_cookies(
    cookies: &Cookies,
    public_key: &PublicKey,
) -> Option<SessionSecret> {
    let value = cookies
        .get(&public_key.z32())
        .map(|c| c.value().to_string())?;
    SessionSecret::new(value).ok()
}

/// Resolved authentication context — inserted into request extensions by the
/// authentication middleware. Handlers just add `auth: AuthSession` as a parameter.
#[derive(Clone, Debug)]
pub enum AuthSession {
    /// Deprecated cookie-based session.
    Cookie(CookieSession),
    /// Grant-based JWT Bearer token session.
    Bearer(BearerSession),
}

/// Deprecated cookie-based session data.
#[derive(Clone, Debug)]
pub struct CookieSession {
    /// The session entity from the database.
    pub session: SessionEntity,
}

/// Grant-based JWT Bearer token session data.
#[derive(Clone, Debug)]
pub struct BearerSession {
    /// User public key.
    pub user_key: PublicKey,
    /// Capabilities from the underlying grant.
    pub capabilities: Capabilities,
    /// Grant ID (for revocation).
    pub grant_id: GrantId,
    /// Token ID (session cache key).
    pub token_id: TokenId,
}

impl AuthSession {
    /// Capabilities regardless of auth method.
    pub fn capabilities(&self) -> &Capabilities {
        match self {
            AuthSession::Cookie(c) => &c.session.capabilities,
            AuthSession::Bearer(b) => &b.capabilities,
        }
    }

    /// User public key regardless of auth method.
    pub fn user_key(&self) -> &PublicKey {
        match self {
            AuthSession::Cookie(c) => &c.session.user_pubkey,
            AuthSession::Bearer(b) => &b.user_key,
        }
    }
}

impl<S> FromRequestParts<S> for AuthSession
where
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthSession>()
            .cloned()
            .ok_or((
                StatusCode::UNAUTHORIZED,
                "No authenticated session found",
            ))
            .map_err(|e| e.into_response())
    }
}

// ── Layer ───────────────────────────────────────────────────────────────────

/// Tower layer that resolves credentials into an [`AuthSession`].
///
/// Inserts an `AuthSession` into request extensions when authentication
/// succeeds. Rejects with 401 if a Bearer token is present but invalid.
/// Requests without credentials or with invalid cookies are forwarded
/// without an identity for downstream layers to handle.
#[derive(Debug, Clone)]
pub struct AuthenticationLayer {
    state: AppState,
}

impl AuthenticationLayer {
    pub fn new(state: AppState) -> Self {
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
    state: AppState,
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
    state: &AppState,
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

/// Authenticate via grant-based JWT Bearer token.
///
/// Returns `Err` with a specific error message if the token is present but invalid.
async fn authenticate_bearer(
    state: &AppState,
    token: &JwsCompact,
) -> Result<AuthSession, HttpError> {
    let jwt = AccessJwt::verify(token, &state.homeserver_keypair.public_key())
        .map_err(|_| HttpError::unauthorized_with_message("Invalid or expired JWT"))?;

    GrantSessionRepository::get_by_token_id(&jwt.token_id, &mut state.sql_db.pool().into())
        .await
        .map_err(|_| HttpError::unauthorized_with_message("Session not found"))?;

    let grant = lookup_active_grant(state, &jwt.grant_id).await?;

    Ok(AuthSession::Bearer(BearerSession {
        user_key: jwt.user_key,
        capabilities: grant.capabilities,
        grant_id: jwt.grant_id,
        token_id: jwt.token_id,
    }))
}

/// Look up a grant and verify it's not revoked or expired.
async fn lookup_active_grant(
    state: &AppState,
    grant_id: &GrantId,
) -> Result<GrantEntity, HttpError> {
    let grant = GrantRepository::get_by_grant_id(grant_id, &mut state.sql_db.pool().into())
        .await
        .map_err(|_| HttpError::unauthorized_with_message("Grant not found"))?;

    if grant.revoked_at.is_some() {
        return Err(HttpError::unauthorized_with_message("Grant has been revoked"));
    }

    let now = chrono::Utc::now().timestamp();
    if grant.expires_at <= now {
        return Err(HttpError::unauthorized_with_message("Grant has expired"));
    }

    Ok(grant)
}

/// Authenticate via deprecated session cookie.
async fn authenticate_cookie(
    state: &AppState,
    cookies: &Cookies,
    public_key: &PublicKey,
) -> Option<AuthSession> {
    let session_secret = session_secret_from_cookies(cookies, public_key)?;

    let session =
        SessionRepository::get_by_secret(&session_secret, &mut state.sql_db.pool().into())
            .await
            .ok()?;

    if &session.user_pubkey != public_key {
        return None;
    }

    Some(AuthSession::Cookie(CookieSession { session }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_context::AppContext;
    use crate::client_server::auth::access_jwt_issuer::AccessJwt;
    use crate::client_server::AppState;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use pubky_common::auth::access_jwt::AccessJwtClaims;
    use pubky_common::auth::jws::{GrantId, TokenId};
    use pubky_common::auth::AuthVerifier;
    use pubky_common::crypto::Keypair;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn test_state() -> AppState {
        let context = AppContext::test().await;
        let quota_mb = context.config_toml.general.user_storage_quota_mb;
        let quota_bytes = if quota_mb == 0 {
            None
        } else {
            Some(quota_mb * 1024 * 1024)
        };
        let auth_service = crate::client_server::auth::AuthService::new(
            context.sql_db.clone(),
            context.keypair.clone(),
        );
        AppState {
            verifier: AuthVerifier::default(),
            sql_db: context.sql_db.clone(),
            file_service: context.file_service.clone(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
            user_quota_bytes: quota_bytes,
            metrics: context.metrics.clone(),
            events_service: context.events_service.clone(),
            homeserver_keypair: context.keypair.clone(),
            auth_service,
        }
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

    /// Mint a valid JWT signed by the given homeserver keypair.
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

    // --- extract_bearer_token ---

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

    // --- middleware: no credentials ---

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn no_credentials_forwards_without_auth_session() {
        let state = test_state().await;
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
        let state = test_state().await;
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
        let state = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        // Mint a JWT signed by a different keypair than the homeserver's
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
        let state = test_state().await;
        let svc = AuthenticationLayer::new(state.clone()).layer(assert_handler(false));

        // Mint a JWT signed by the correct homeserver keypair,
        // but no matching grant session exists in the DB.
        let token = mint_jwt(&state.homeserver_keypair);

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
        let state = test_state().await;
        let svc = AuthenticationLayer::new(state.clone()).layer(assert_handler(false));

        let user_kp = Keypair::random();
        let claims = AccessJwtClaims {
            iss: state.homeserver_keypair.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: 1000,
            exp: 2000, // far in the past
        };
        let token = AccessJwt::mint(&state.homeserver_keypair, &claims);

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
        let state = test_state().await;
        // Non-Bearer Authorization header is rejected with 401.
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
        let state = test_state().await;
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        // Cookie auth requires PubkyHost — without it, silently fails
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
        let svc = AuthenticationLayer::new(state).layer(assert_handler(false));

        let pk = Keypair::random().public_key();
        let mut req = Request::builder()
            .uri("/session")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pk.clone()));
        // Insert a Cookies jar with a fake session secret
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
        let state = test_state().await;
        // Bearer is checked first — if invalid, rejects with 401
        // regardless of any cookie in the request.
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
