//! Authorization middleware.
//!
//! Enforces access control on every request:
//! - **Reads** to `/pub/*` are always allowed (public data).
//! - **Writes** (`PUT`/`DELETE`) require a valid session cookie whose capabilities
//!   grant write access to the target path and whose user matches the tenant.
//! - Non-public paths are forbidden for external requests.
//!
//! When a request has a valid session, an [`AuthenticatedSession`] marker is
//! inserted into the request extensions so downstream layers (e.g. per-user
//! bandwidth budgets) can distinguish authenticated from anonymous requests.

use crate::client_server::{extractors::PubkyHost, AppState};
use crate::persistence::sql::session::{SessionRepository, SessionSecret};
use crate::shared::{HttpError, HttpResult};
use axum::http::Method;
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use pubky_common::crypto::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

/// Marker inserted into request extensions when the request has a valid session.
/// Downstream middleware (e.g. per-user bandwidth budgets) can check for this to
/// distinguish authenticated from anonymous requests.
#[derive(Debug, Clone)]
pub struct AuthenticatedSession;

/// A Tower Layer to handle authorization for write operations.
#[derive(Debug, Clone)]
pub struct AuthorizationLayer {
    state: AppState,
}

impl AuthorizationLayer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for AuthorizationLayer {
    type Service = AuthorizationMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthorizationMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct AuthorizationMiddleware<S> {
    inner: S,
    state: AppState,
}

impl<S> Service<Request<Body>> for AuthorizationMiddleware<S>
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

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let state = self.state.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let path = req.uri().path();

            let pubky = match req.extensions().get::<PubkyHost>() {
                Some(pk) => pk,
                None => {
                    tracing::warn!("Pubky Host is missing in request. Authorization failed.");
                    return Ok(HttpError::new_with_message(
                        StatusCode::NOT_FOUND,
                        "Pubky Host is missing",
                    )
                    .into_response());
                }
            };

            let cookies = match req.extensions().get::<Cookies>() {
                Some(cookies) => cookies,
                None => {
                    tracing::warn!("No cookies found in request. Unauthorized.");
                    return Ok(HttpError::unauthorized().into_response());
                }
            };

            // Authorize the request
            match authorize(&state, req.method(), cookies, pubky.public_key(), path).await {
                Err(e) => return Ok(e.into_response()),
                Ok(AuthzResult::Authenticated) => {
                    req.extensions_mut().insert(AuthenticatedSession);
                }
                Ok(AuthzResult::Anonymous) => {}
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Result of the authorization check.
enum AuthzResult {
    /// A valid session was found and verified.
    Authenticated,
    /// The request is allowed without a session (or no valid session was present).
    Anonymous,
}

/// Authorize the request. For paths that don't require a session (public reads,
/// `/session`), a lightweight session probe is still performed so that
/// downstream layers (e.g. per-user bandwidth budgets) can identify the user
/// — but only when a session cookie is actually present in the request.
async fn authorize(
    state: &AppState,
    method: &Method,
    cookies: &Cookies,
    public_key: &PublicKey,
    path: &str,
) -> HttpResult<AuthzResult> {
    if path == "/session" {
        // Checking (or deleting) one's session is ok for everyone
        return Ok(AuthzResult::Anonymous);
    } else if path.starts_with("/pub/") {
        if method == Method::GET || method == Method::HEAD {
            return Ok(probe_session(state, cookies, public_key).await);
        }
    } else if path.starts_with("/dav/") {
        // XXX: at least for now
        // if method == Method::GET {
        //     return Ok(false);
        // }
    } else {
        tracing::warn!(
            "Writing to directories other than '/pub/' is forbidden: {}/{}. Access forbidden",
            public_key,
            path
        );
        return Err(HttpError::forbidden_with_message(
            "Writing to directories other than '/pub/' is forbidden",
        ));
    }

    let session_secret = match session_secret_from_cookies(cookies, public_key) {
        Some(session_secret) => session_secret,
        None => {
            tracing::warn!(
                "No session secret found in cookies for pubky-host: {}",
                public_key
            );
            return Err(HttpError::unauthorized_with_message(
                "No session secret found in cookies",
            ));
        }
    };

    let session =
        match SessionRepository::get_by_secret(&session_secret, &mut state.sql_db.pool().into())
            .await
        {
            Ok(session) => session,
            Err(sqlx::Error::RowNotFound) => {
                tracing::warn!(
                    "No session found in the database for session secret: {}, pubky: {}",
                    session_secret,
                    public_key
                );
                return Err(HttpError::unauthorized_with_message(
                    "No session found for session secret",
                ));
            }
            Err(e) => return Err(e.into()),
        };

    if &session.user_pubkey != public_key {
        tracing::warn!(
            "SessionInfo public key does not match pubky-host: {} != {}",
            session.user_pubkey,
            public_key
        );
        return Err(HttpError::unauthorized_with_message(
            "SessionInfo public key does not match pubky-host",
        ));
    }

    if session.capabilities.iter().any(|cap| {
        path.starts_with(&cap.scope)
            && cap
                .actions
                .contains(&pubky_common::capabilities::Action::Write)
    }) {
        Ok(AuthzResult::Authenticated)
    } else {
        tracing::warn!(
            "SessionInfo {} pubkey {} does not have write access to {}. Access forbidden",
            session_secret,
            public_key,
            path
        );
        Err(HttpError::forbidden_with_message(
            "Session does not have write access to path",
        ))
    }
}

/// Lightweight session probe: if the request carries a valid session cookie for
/// this pubkey, return `Authenticated`; otherwise `Anonymous`.
/// Only performs a DB lookup when a session cookie is actually present, so
/// anonymous requests incur no overhead.
async fn probe_session(state: &AppState, cookies: &Cookies, public_key: &PublicKey) -> AuthzResult {
    let Some(session_secret) = session_secret_from_cookies(cookies, public_key) else {
        return AuthzResult::Anonymous;
    };
    match SessionRepository::get_by_secret(&session_secret, &mut state.sql_db.pool().into()).await {
        Ok(session) if &session.user_pubkey == public_key => AuthzResult::Authenticated,
        _ => AuthzResult::Anonymous,
    }
}

/// Get the session secret from the cookies.
/// Returns None if the session secret is not found or invalid.
pub fn session_secret_from_cookies(
    cookies: &Cookies,
    public_key: &PublicKey,
) -> Option<SessionSecret> {
    let value = cookies
        .get(&public_key.z32())
        .map(|c| c.value().to_string())?;
    SessionSecret::new(value).ok()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::{Extension, Router};
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_common::crypto::Keypair;
    use tower::ServiceExt;
    use tower_cookies::CookieManagerLayer;

    use crate::app_context::AppContext;
    use crate::client_server::extractors::PubkyHost;
    use crate::persistence::sql::session::SessionRepository;
    use crate::persistence::sql::user::UserRepository;

    use pubky_common::auth::AuthVerifier;

    use super::*;

    /// Handler that checks whether `AuthenticatedSession` was inserted.
    async fn check_auth_marker(
        marker: Option<Extension<AuthenticatedSession>>,
    ) -> impl IntoResponse {
        if marker.is_some() {
            (StatusCode::OK, "authenticated")
        } else {
            (StatusCode::OK, "anonymous")
        }
    }

    /// Build a minimal app with the auth layer around a test handler.
    fn build_auth_app(state: AppState) -> Router {
        Router::new()
            .route("/pub/data", get(check_auth_marker))
            .layer(AuthorizationLayer::new(state))
            .layer(CookieManagerLayer::new())
    }

    fn make_get_request(pubkey: &PublicKey, session_cookie: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(Method::GET).uri("/pub/data");
        if let Some(cookie_val) = session_cookie {
            builder = builder.header("cookie", format!("{}={}", pubkey.z32(), cookie_val));
        }
        let mut req = builder.body(Body::empty()).unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));
        req
    }

    fn test_app_state(context: &AppContext) -> AppState {
        use crate::persistence::files::FileService;
        AppState {
            verifier: AuthVerifier::default(),
            sql_db: context.sql_db.clone(),
            file_service: FileService::new_from_context(context).unwrap(),
            signup_mode: context.config_toml.general.signup_mode.clone(),
            metrics: context.metrics.clone(),
            events_service: context.events_service.clone(),
            default_user_limits: Default::default(),
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_public_read_without_cookie_is_anonymous() {
        let context = AppContext::test().await;
        let app = build_auth_app(test_app_state(&context));
        let pubkey = Keypair::random().public_key();

        let resp = app.oneshot(make_get_request(&pubkey, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(body, "anonymous");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_public_read_with_valid_cookie_is_authenticated() {
        let context = AppContext::test().await;
        let state = test_app_state(&context);
        let app = build_auth_app(state);

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user = UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        let caps = Capabilities::builder().cap(Capability::root()).finish();
        let secret = SessionRepository::create(user.id, &caps, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let resp = app
            .oneshot(make_get_request(&pubkey, Some(&secret.to_string())))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(body, "authenticated");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_public_read_with_invalid_cookie_is_anonymous() {
        let context = AppContext::test().await;
        let app = build_auth_app(test_app_state(&context));
        let pubkey = Keypair::random().public_key();

        // Use a bogus session secret
        let resp = app
            .oneshot(make_get_request(
                &pubkey,
                Some("bogus-secret-value-000000000000000000000"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(body, "anonymous");
    }
}
