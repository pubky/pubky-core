//! Authorization checks for write requests.
//!
//! [`has_write_permission`] is a pure predicate — it answers "may this session
//! write this path on this tenant?" without touching axum, request extensions,
//! or any framework concern. Authentication (does a session exist?) is the
//! [`AuthenticationLayer`]'s job; this module only does authorization, so a
//! failure here is always a 403, never a 401.
//!
//! Handlers extract [`AuthSession`] and [`PubkyHost`] as normal arguments and
//! call this function explicitly before performing the write.
//!
//! [`AuthenticationLayer`]: super::AuthenticationLayer

use pubky_common::capabilities::Action;

use crate::client_server::auth::AuthSession;
use crate::client_server::middleware::pubky_host::PubkyHost;
use crate::shared::webdav::WebDavPath;
use crate::shared::HttpError;

/// Storage roots a write may target.
/// `/pub/` is public readable
/// `/priv/` is private storage
const WRITABLE_ROOTS: [&str; 2] = ["/pub/", "/priv/"];

/// Authorize a write to `path` for `session` on tenant `pubky`.
///
/// Returns `Ok(())` when the path is under one of [`WRITABLE_ROOTS`], the
/// session targets the same tenant, and the session holds a capability whose
/// scope covers `path` with [`Action::Write`]. Returns a 403 `HttpError`
/// otherwise.
///
/// The writable root requirement is enforced here (not at the path extractor)
/// so that violations produce a 403 with a meaningful message — the SDK
/// contract expects `"Writing to directories other than '/pub/' and '/priv/'
/// is forbidden"` rather than axum's default 400 for a deserialization failure.
pub fn has_write_permission(
    session: &AuthSession,
    pubky: &PubkyHost,
    path: &WebDavPath,
) -> Result<(), HttpError> {
    let path_str = path.as_str();

    if !WRITABLE_ROOTS.iter().any(|root| path_str.starts_with(root)) {
        return Err(HttpError::forbidden_with_message(
            "Writing to directories other than '/pub/' and '/priv/' is forbidden",
        ));
    }

    if session.user_key() != pubky.public_key() {
        return Err(HttpError::forbidden_with_message(
            "Session user does not match target tenant",
        ));
    }

    let granted = session
        .capabilities()
        .iter()
        .any(|cap| cap.scope_covers_path(path_str) && cap.actions.contains(&Action::Write));

    if granted {
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
    use crate::client_server::auth::grant::session::GrantSession;
    use pubky_common::auth::jws::GrantId;
    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_common::crypto::{Keypair, PublicKey};

    fn dummy_pk() -> PublicKey {
        Keypair::random().public_key()
    }

    fn web_path(s: &str) -> WebDavPath {
        WebDavPath::new(s).expect("test path must be a syntactically valid webdav path")
    }

    fn session_with_key(pk: PublicKey, capabilities: Capabilities) -> AuthSession {
        AuthSession::Grant(GrantSession {
            user_key: pk,
            capabilities,
            grant_id: GrantId::generate(),
            token_expires_at: 9999999999,
        })
    }

    fn session_with_caps(capabilities: Capabilities) -> (AuthSession, PubkyHost) {
        let pk = dummy_pk();
        let session = session_with_key(pk.clone(), capabilities);
        (session, PubkyHost(pk))
    }

    fn root_caps() -> Capabilities {
        Capabilities::from(vec![Capability::root()])
    }

    fn scoped_caps(scope: &str) -> Capabilities {
        Capabilities::from(vec![Capability::write(scope)])
    }

    fn read_only_caps() -> Capabilities {
        Capabilities::from(vec![Capability::read("/")])
    }

    #[test]
    fn root_capability_grants_access_to_any_pub_path() {
        let (session, pubky) = session_with_caps(root_caps());
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/anything")).is_ok());
    }

    #[test]
    fn empty_capabilities_denies_access() {
        let (session, pubky) = session_with_caps(Capabilities::from(vec![]));
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/file")).is_err());
    }

    #[test]
    fn read_only_capabilities_deny_write() {
        let (session, pubky) = session_with_caps(read_only_caps());
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/file.txt")).is_err());
    }

    #[test]
    fn scoped_capability_grants_access_to_subpath() {
        let (session, pubky) = session_with_caps(scoped_caps("/pub/my.app/"));
        assert!(
            has_write_permission(&session, &pubky, &web_path("/pub/my.app/nested/file")).is_ok()
        );
    }

    #[test]
    fn scoped_capability_denies_access_to_sibling_path() {
        let (session, pubky) = session_with_caps(scoped_caps("/pub/my.app/"));
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/other.app/file")).is_err());
    }

    #[test]
    fn scoped_capability_without_slash_rejects_prefix_attack() {
        let (session, pubky) = session_with_caps(scoped_caps("/pub/app"));
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/app-evil/file")).is_err());
    }

    #[test]
    fn scoped_capability_without_slash_allows_exact_match() {
        let (session, pubky) = session_with_caps(scoped_caps("/pub/app"));
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/app")).is_ok());
    }

    #[test]
    fn directory_scope_denies_write_to_directory_path_without_trailing_slash() {
        // Regression for the e2e auth tests (`tests::auth::authz`,
        // `signup_authz`, `authz_timeout_reconnect`): a capability granted
        // for `/pub/pubky.app/` (the directory) must NOT cover a write to
        // `/pub/pubky.app` (treated as a file at the parent level).
        let (session, pubky) = session_with_caps(scoped_caps("/pub/pubky.app/"));
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/pubky.app")).is_err());
    }

    #[test]
    fn file_scope_denies_write_to_descendant() {
        // A file scope (no trailing `/`) is not a directory namespace —
        // granting `/pub/app:rw` does not authorize writes to `/pub/app/foo`.
        let (session, pubky) = session_with_caps(scoped_caps("/pub/app"));
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/app/foo")).is_err());
    }

    #[test]
    fn cross_tenant_write_is_rejected() {
        // Session owned by user A, target tenant is user B.
        let session = session_with_key(dummy_pk(), root_caps());
        let pubky = PubkyHost(dummy_pk());
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/file.txt")).is_err());
    }

    #[test]
    fn same_tenant_write_with_root_caps_is_allowed() {
        let pk = dummy_pk();
        let session = session_with_key(pk.clone(), root_caps());
        let pubky = PubkyHost(pk);
        assert!(has_write_permission(&session, &pubky, &web_path("/pub/file.txt")).is_ok());
    }

    #[test]
    fn write_outside_writable_roots_is_rejected() {
        // SDK contract: writes to roots other than `/pub/` and `/priv/` must
        // return 403 with a message containing `"Writing to directories other
        // than '/pub/' and '/priv/'"`. The HTTP shape is
        // covered end-to-end by that SDK test; here we just verify the
        // predicate rejects the path before any tenant/capability check.
        let (session, pubky) = session_with_caps(root_caps());
        assert!(has_write_permission(&session, &pubky, &web_path("/foo/example.com/x")).is_err());
    }

    #[test]
    fn root_capability_grants_access_to_any_priv_path() {
        let (session, pubky) = session_with_caps(root_caps());
        assert!(has_write_permission(&session, &pubky, &web_path("/priv/anything")).is_ok());
    }

    #[test]
    fn priv_path_with_covering_cap_is_allowed() {
        // A write cap scoped to `/priv/app/` authorizes writes beneath it,
        // exactly as it does under `/pub/`.
        let (session, pubky) = session_with_caps(scoped_caps("/priv/app/"));
        assert!(has_write_permission(&session, &pubky, &web_path("/priv/app/x")).is_ok());
    }

    #[test]
    fn priv_path_with_only_pub_caps_is_denied() {
        // A `/pub/`-scoped cap does not cover a `/priv/` write. Uses a scoped
        // cap rather than root, since a root `/` cap would cover `/priv/` too.
        let (session, pubky) = session_with_caps(scoped_caps("/pub/app/"));
        assert!(has_write_permission(&session, &pubky, &web_path("/priv/app/x")).is_err());
    }
}
