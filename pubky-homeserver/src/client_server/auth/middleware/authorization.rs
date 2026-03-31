//! Authorization extractor — enforces capability-based access control on write routes.
//!
//! The [`WriteAccess`] extractor validates that the request has a valid
//! [`AuthSession`] with write capabilities matching the target path.
//! Add it as a parameter to any handler that requires write authorization.
//!
//! Read routes do not use this extractor — public reads on `/pub/*` need no
//! authentication, and the [`AuthenticationLayer`] already rejects invalid
//! Bearer tokens.

use super::authentication::AuthSession;
use crate::shared::HttpError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use pubky_common::capabilities::Action;

/// Proof of write authorization — extracted by write handlers.
///
/// Validates that the request has a valid [`AuthSession`] whose capabilities
/// grant write access to the request path. Handlers that need the session
/// can access it via [`WriteAccess::session`].
///
/// # Example
///
/// ```rust,ignore
/// // Authorization enforced by extraction — returns 401/403 before the
/// // handler body runs if credentials are missing or insufficient.
/// async fn put(
///     State(state): State<AppState>,
///     _write: WriteAccess,
///     Path(path): Path<WebDavPathPubAxum>,
///     body: Body,
/// ) -> HttpResult<impl IntoResponse> {
///     // Only reached with a valid, authorized session.
///     // Access session info if needed: `_write.session.user_key()`
///     todo!()
/// }
/// ```
#[derive(Clone, Debug)]
pub struct WriteAccess {
    pub session: AuthSession,
}

impl<S> FromRequestParts<S> for WriteAccess
where
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session = parts
            .extensions
            .get::<AuthSession>()
            .cloned()
            .ok_or_else(|| HttpError::unauthorized().into_response())?;

        let path = parts.uri.path();

        check_writable_path(path).map_err(|e| e.into_response())?;
        check_capabilities(session.capabilities(), path).map_err(|e| e.into_response())?;

        Ok(WriteAccess { session })
    }
}

/// Validate that the path is in a writable directory.
fn check_writable_path(path: &str) -> Result<(), HttpError> {
    if path.starts_with("/pub/") || path.starts_with("/dav/") {
        Ok(())
    } else {
        tracing::warn!(
            "Writing to directories other than '/pub/' is forbidden: {}",
            path
        );
        Err(HttpError::forbidden_with_message(
            "Writing to directories other than '/pub/' is forbidden",
        ))
    }
}

/// Check if capabilities grant write access to the given path.
fn check_capabilities(
    capabilities: &pubky_common::capabilities::Capabilities,
    path: &str,
) -> Result<(), HttpError> {
    let has_access = capabilities
        .iter()
        .any(|cap| path.starts_with(&cap.scope) && cap.actions.contains(&Action::Write));

    if has_access {
        Ok(())
    } else {
        Err(HttpError::forbidden_with_message(
            "Session does not have write access to path",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_server::auth::middleware::authentication::BearerSession;
    use axum::body::Body;
    use axum::http::Request;
    use pubky_common::auth::jws::{GrantId, TokenId};
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_common::crypto::{Keypair, PublicKey};

    fn dummy_pk() -> PublicKey {
        Keypair::random().public_key()
    }

    fn bearer_session_with_caps(capabilities: Capabilities) -> AuthSession {
        AuthSession::Bearer(BearerSession {
            user_key: dummy_pk(),
            capabilities,
            grant_id: GrantId::generate(),
            token_id: TokenId::generate(),
        })
    }

    fn root_caps() -> Capabilities {
        Capabilities(vec![Capability::root()])
    }

    fn scoped_caps(scope: &str) -> Capabilities {
        Capabilities(vec![Capability::write(scope)])
    }

    fn read_only_caps() -> Capabilities {
        Capabilities(vec![Capability::read("/")])
    }

    /// Build request parts with an optional AuthSession in extensions.
    fn parts_with_auth(uri: &str, auth: Option<AuthSession>) -> Parts {
        let (mut parts, _body) = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        if let Some(a) = auth {
            parts.extensions.insert(a);
        }
        parts
    }

    // --- WriteAccess extractor tests ---

    #[tokio::test]
    async fn extractor_rejects_without_auth() {
        let mut parts = parts_with_auth("/pub/file.txt", None);
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn extractor_allows_write_with_root_caps() {
        let auth = bearer_session_with_caps(root_caps());
        let mut parts = parts_with_auth("/pub/file.txt", Some(auth));
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn extractor_rejects_write_with_wrong_scope() {
        let auth = bearer_session_with_caps(scoped_caps("/pub/other.app"));
        let mut parts = parts_with_auth("/pub/my.app/data.json", Some(auth));
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn extractor_rejects_write_to_non_writable_path() {
        let auth = bearer_session_with_caps(root_caps());
        let mut parts = parts_with_auth("/other/file", Some(auth));
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn extractor_allows_write_to_dav() {
        let auth = bearer_session_with_caps(root_caps());
        let mut parts = parts_with_auth("/dav/file.txt", Some(auth));
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn extractor_allows_scoped_write_matching_path() {
        let auth = bearer_session_with_caps(scoped_caps("/pub/my.app"));
        let mut parts = parts_with_auth("/pub/my.app/data.json", Some(auth));
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn extractor_rejects_read_only_caps() {
        let auth = bearer_session_with_caps(read_only_caps());
        let mut parts = parts_with_auth("/pub/file.txt", Some(auth));
        let result = WriteAccess::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    // --- check_writable_path unit tests ---

    #[test]
    fn pub_path_is_writable() {
        assert!(check_writable_path("/pub/file.txt").is_ok());
    }

    #[test]
    fn dav_path_is_writable() {
        assert!(check_writable_path("/dav/file.txt").is_ok());
    }

    #[test]
    fn other_path_is_not_writable() {
        assert!(check_writable_path("/other/file").is_err());
    }

    #[test]
    fn session_path_is_not_writable() {
        assert!(check_writable_path("/session").is_err());
    }

    // --- check_capabilities unit tests ---

    #[test]
    fn root_capability_grants_access_to_any_path() {
        assert!(check_capabilities(&root_caps(), "/pub/anything").is_ok());
    }

    #[test]
    fn empty_capabilities_denies_access() {
        let caps = Capabilities(vec![]);
        assert!(check_capabilities(&caps, "/pub/file").is_err());
    }

    #[test]
    fn scoped_capability_grants_access_to_subpath() {
        let caps = scoped_caps("/pub/my.app");
        assert!(check_capabilities(&caps, "/pub/my.app/nested/file").is_ok());
    }

    #[test]
    fn scoped_capability_denies_access_to_sibling_path() {
        let caps = scoped_caps("/pub/my.app");
        assert!(check_capabilities(&caps, "/pub/other.app/file").is_err());
    }
}
