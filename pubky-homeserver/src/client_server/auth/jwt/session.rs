//! Grant-based JWT session — the resolved session attached to authenticated requests.

use pubky_common::auth::jws::{GrantId, TokenId};
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;

/// Grant-based JWT session data.
///
/// Constructed by [`AuthService::resolve_grant_session`](super::service::AuthService::resolve_grant_session)
/// and wrapped in [`AuthSession::Grant`](crate::client_server::auth::AuthSession::Grant)
/// by the JWT authentication middleware.
#[derive(Clone, Debug)]
pub struct GrantSession {
    /// User public key.
    pub user_key: PublicKey,
    /// Capabilities from the underlying grant.
    pub capabilities: Capabilities,
    /// Grant ID (for revocation).
    pub grant_id: GrantId,
    /// Token ID (session cache key).
    pub token_id: TokenId,
    /// When the JWT token expires (Unix seconds).
    pub token_expires_at: u64,
}
