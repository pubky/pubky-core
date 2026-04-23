//! Grant-based session — the resolved session attached to authenticated requests.

use pubky_common::auth::jws::GrantId;
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;

/// Grant-based session data.
///
/// Constructed by [`AuthService::resolve_grant_session_by_bearer`](super::service::AuthService::resolve_grant_session_by_bearer)
/// and wrapped in [`AuthSession::Grant`](crate::client_server::auth::AuthSession::Grant)
/// by the authentication middleware.
#[derive(Clone, Debug)]
pub struct GrantSession {
    /// User public key.
    pub user_key: PublicKey,
    /// Capabilities from the underlying grant.
    pub capabilities: Capabilities,
    /// Grant ID (for revocation).
    pub grant_id: GrantId,
    /// When the bearer expires (Unix seconds).
    pub token_expires_at: u64,
}
