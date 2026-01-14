use crate::client_server::{extractors::PubkyHost, AppState};
use crate::persistence::sql::session::{SessionRepository, SessionSecret};
use crate::persistence::sql::SqlDb;
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

    fn call(&mut self, req: Request<Body>) -> Self::Future {
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
            if let Err(e) = authorize(
                &state.sql_db,
                req.method(),
                cookies,
                pubky.public_key(),
                path,
            )
            .await
            {
                return Ok(e.into_response());
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Authorize request.
async fn authorize(
    sql_db: &SqlDb,
    method: &Method,
    cookies: &Cookies,
    public_key: &PublicKey,
    path: &str,
) -> HttpResult<()> {
    if path == "/session" {
        // Checking (or deleting) one's session is ok for everyone
        return Ok(());
    } else if path.starts_with("/pub/") {
        if method == Method::GET || method == Method::HEAD {
            return Ok(());
        }
    } else if path.starts_with("/dav/") {
        // XXX: at least for now
        // if method == Method::GET {
        //     return Ok(());
        // }
    } else {
        tracing::warn!(
            "Access to non-/pub/ paths is forbidden: {}/{}.",
            public_key,
            path
        );
        return Err(HttpError::forbidden_with_message(
            "Access to non-/pub/ paths is forbidden",
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
        match SessionRepository::get_by_secret(&session_secret, &mut sql_db.pool().into()).await {
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
        Ok(())
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
pub mod tests {
    use std::str::FromStr;

    use pkarr::{Keypair, PublicKey};
    use pubky_common::capabilities::{Capabilities, Capability};
    use reqwest::{Method, StatusCode};
    use tower_cookies::{Cookie, Cookies};

    use crate::{
        client_server::layers::authz::authorize,
        persistence::sql::{session::SessionRepository, user::UserRepository, SqlDb},
    };

    const PUBKEY: &str = "o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy";

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_non_pub_paths() {
        let methods = vec![
            Method::GET,
            Method::PUT,
            Method::POST,
            Method::DELETE,
            Method::PATCH,
        ];

        let db = SqlDb::test().await;
        let cookies = Cookies::default();
        let public_key = PublicKey::from_str(PUBKEY).unwrap();

        for method in methods {
            let result = authorize(&db, &method, &cookies, &public_key, "/test").await;
            match result {
                Err(http) => {
                    assert_eq!(
                        http.status(),
                        StatusCode::FORBIDDEN,
                        "Method {:?} on /test",
                        method
                    );
                    assert_eq!(
                        http.detail(),
                        Some("Access to non-/pub/ paths is forbidden"),
                        "Error message should indicate non-/pub/ path forbidden"
                    );
                }
                Ok(_) => panic!("Expected error for method {:?} on /test, got Ok", method),
            }
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_pub_paths() {
        let test_cases = vec![
            (Method::GET, None),
            (Method::HEAD, None),
            (Method::PUT, Some(StatusCode::UNAUTHORIZED)),
            (Method::POST, Some(StatusCode::UNAUTHORIZED)),
            (Method::DELETE, Some(StatusCode::UNAUTHORIZED)),
            (Method::PATCH, Some(StatusCode::UNAUTHORIZED)),
        ];

        let db = SqlDb::test().await;
        let cookies = Cookies::default();
        let public_key = PublicKey::from_str(PUBKEY).unwrap();

        for (method, expected_error) in test_cases {
            let result = authorize(&db, &method, &cookies, &public_key, "/pub/test").await;
            match expected_error {
                Some(expected_status) => match result {
                    Err(http) => {
                        assert_eq!(
                            http.status(),
                            expected_status,
                            "Method {:?} on /pub/test",
                            method
                        );
                        if expected_status == StatusCode::UNAUTHORIZED {
                            assert_eq!(
                                http.detail(),
                                Some("No session secret found in cookies"),
                                "Error message should indicate missing session cookie"
                            );
                        }
                    }
                    Ok(_) => panic!(
                        "Expected error {:?} for method {:?} on /pub/test, got Ok",
                        expected_status, method
                    ),
                },
                None => {
                    if let Err(http) = result {
                        panic!(
                            "Expected Ok for method {:?} on /pub/test, got error {:?}",
                            method,
                            http.status()
                        );
                    }
                }
            }
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_session_path_allows_all_methods() {
        let methods = vec![
            Method::GET,
            Method::PUT,
            Method::POST,
            Method::DELETE,
            Method::PATCH,
        ];

        let db = SqlDb::test().await;
        let cookies = Cookies::default();
        let public_key = PublicKey::from_str(PUBKEY).unwrap();

        for method in methods {
            let result = authorize(&db, &method, &cookies, &public_key, "/session").await;
            assert!(
                result.is_ok(),
                "Method {:?} on /session should be allowed without auth",
                method
            );
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_valid_session_with_write_capability() {
        let db = SqlDb::test().await;
        let keypair = Keypair::random();
        let public_key = keypair.public_key();

        // Create user
        UserRepository::create(&public_key, &mut db.pool().into())
            .await
            .unwrap();

        // Create session with root capability (write access to /pub/)
        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let user = UserRepository::get(&public_key, &mut db.pool().into())
            .await
            .unwrap();
        let session_secret =
            SessionRepository::create(user.id, &capabilities, &mut db.pool().into())
                .await
                .unwrap();

        // Create cookies with session secret
        let cookies = Cookies::default();
        cookies.add(Cookie::new(
            public_key.to_string(),
            session_secret.to_string(),
        ));

        // Test write operations should succeed
        let write_methods = vec![Method::PUT, Method::POST, Method::DELETE, Method::PATCH];

        for method in write_methods {
            let result = authorize(&db, &method, &cookies, &public_key, "/pub/test.txt").await;
            assert!(
                result.is_ok(),
                "Method {:?} on /pub/test.txt with valid session should succeed",
                method
            );
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_session_pubkey_mismatch() {
        let db = SqlDb::test().await;
        let keypair = Keypair::random();
        let public_key = keypair.public_key();

        // Create user and session
        UserRepository::create(&public_key, &mut db.pool().into())
            .await
            .unwrap();

        let capabilities = Capabilities::builder().cap(Capability::root()).finish();
        let user = UserRepository::get(&public_key, &mut db.pool().into())
            .await
            .unwrap();
        let session_secret =
            SessionRepository::create(user.id, &capabilities, &mut db.pool().into())
                .await
                .unwrap();

        // Create cookies with session secret but use different public key
        let different_keypair = Keypair::random();
        let different_public_key = different_keypair.public_key();

        let cookies = Cookies::default();
        cookies.add(Cookie::new(
            different_public_key.to_string(),
            session_secret.to_string(),
        ));

        // Should fail with unauthorized because pubkey doesn't match session
        let result = authorize(
            &db,
            &Method::PUT,
            &cookies,
            &different_public_key,
            "/pub/test.txt",
        )
        .await;

        match result {
            Err(http) => {
                assert_eq!(
                    http.status(),
                    StatusCode::UNAUTHORIZED,
                    "Pubkey mismatch should return UNAUTHORIZED"
                );
                assert_eq!(
                    http.detail(),
                    Some("SessionInfo public key does not match pubky-host"),
                    "Error message should indicate pubkey mismatch"
                );
            }
            Ok(_) => panic!("Expected UNAUTHORIZED for pubkey mismatch, got Ok"),
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_session_without_write_capability() {
        let db = SqlDb::test().await;
        let keypair = Keypair::random();
        let public_key = keypair.public_key();

        // Create user
        UserRepository::create(&public_key, &mut db.pool().into())
            .await
            .unwrap();

        // Create session with limited capability (only read access to specific path)
        let capabilities = Capabilities::builder()
            .cap(Capability::read("/pub/readonly/"))
            .finish();
        let user = UserRepository::get(&public_key, &mut db.pool().into())
            .await
            .unwrap();
        let session_secret =
            SessionRepository::create(user.id, &capabilities, &mut db.pool().into())
                .await
                .unwrap();

        // Create cookies with session secret
        let cookies = Cookies::default();
        cookies.add(Cookie::new(
            public_key.to_string(),
            session_secret.to_string(),
        ));

        // Try to write to /pub/test.txt (should fail - no write capability)
        let result = authorize(&db, &Method::PUT, &cookies, &public_key, "/pub/test.txt").await;

        match result {
            Err(http) => {
                assert_eq!(
                    http.status(),
                    StatusCode::FORBIDDEN,
                    "Write without write capability should return FORBIDDEN"
                );
                assert_eq!(
                    http.detail(),
                    Some("Session does not have write access to path"),
                    "Error message should indicate missing write capability"
                );
            }
            Ok(_) => panic!("Expected FORBIDDEN for write without capability, got Ok"),
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_invalid_session_secret_in_db() {
        let db = SqlDb::test().await;
        let keypair = Keypair::random();
        let public_key = keypair.public_key();

        // Create user but no session
        UserRepository::create(&public_key, &mut db.pool().into())
            .await
            .unwrap();

        // Create cookies with non-existent session secret (must be 26 chars)
        let cookies = Cookies::default();
        cookies.add(Cookie::new(
            public_key.to_string(),
            "abcdefghijklmnopqrstuvwxyz", // 26 chars, valid format but not in DB
        ));

        // Should fail with unauthorized because session doesn't exist in DB
        let result = authorize(&db, &Method::PUT, &cookies, &public_key, "/pub/test.txt").await;

        match result {
            Err(http) => {
                assert_eq!(
                    http.status(),
                    StatusCode::UNAUTHORIZED,
                    "Invalid session secret should return UNAUTHORIZED"
                );
                assert_eq!(
                    http.detail(),
                    Some("No session found for session secret"),
                    "Error message should indicate session not found in database"
                );
            }
            Ok(_) => panic!("Expected UNAUTHORIZED for invalid session secret, got Ok"),
        }
    }
}
