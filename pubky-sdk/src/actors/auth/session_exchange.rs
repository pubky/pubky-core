use std::fmt;

use pubky_common::crypto::{Keypair, PublicKey};

use crate::errors::{AuthError, Result};
#[allow(deprecated, reason = "Internal use of deprecated public API")]
use crate::{PubkyHttpClient, PubkySession, actors::Pkdns};

use super::approval::AuthApproval;

/// Context required to convert a grant approval into a JWT-backed [`PubkySession`].
#[derive(Clone)]
pub(crate) struct GrantSessionContext {
    /// Client (`PoP`) keypair bound by the grant's `cnf` claim.
    pub client_keypair: Keypair,
    /// For sign-up flows: the homeserver to create the user on, plus an
    /// optional signup token. For sign-in flows this is `None` and the
    /// homeserver is resolved from PKARR after the grant arrives.
    pub signup_homeserver: Option<(PublicKey, Option<String>)>,
}

impl fmt::Debug for GrantSessionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrantSessionContext")
            .field("client_keypair", &"<redacted>")
            .field(
                "signup_homeserver",
                &self.signup_homeserver.as_ref().map(|(pk, _)| pk.z32()),
            )
            .finish()
    }
}

/// Convert an [`AuthApproval`] into a fully hydrated [`PubkySession`].
pub(crate) async fn session_from_approval(
    client: PubkyHttpClient,
    grant_ctx: Option<GrantSessionContext>,
    approval: AuthApproval,
) -> Result<PubkySession> {
    match approval {
        AuthApproval::Legacy(token) => {
            crate::actors::session::cookie::session_from_auth_token(&token, client).await
        }
        AuthApproval::Grant { jws, claims } => {
            let ctx = grant_ctx.ok_or_else(|| {
                AuthError::Validation(
                    "received a grant payload but no client keypair is configured".into(),
                )
            })?;
            let claims = *claims;
            if let Some((hs_pk, signup_token)) = ctx.signup_homeserver {
                crate::actors::session::jwt::session_from_grant_signup(
                    client,
                    jws,
                    claims,
                    ctx.client_keypair,
                    hs_pk,
                    signup_token.as_deref(),
                )
                .await
            } else {
                // Sign-in: resolve the user's homeserver via PKARR.
                let pkdns = Pkdns::with_client(client.clone());
                let hs_pk = pkdns.get_homeserver_of(&claims.iss).await.ok_or_else(|| {
                    AuthError::Validation(format!(
                        "could not resolve homeserver for {}",
                        claims.iss.z32()
                    ))
                })?;
                crate::actors::session::jwt::session_from_grant_exchange(
                    client,
                    jws,
                    claims,
                    ctx.client_keypair,
                    hs_pk,
                )
                .await
            }
        }
    }
}
