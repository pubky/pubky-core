use std::fmt;
use pubky_common::{
    auth::grant::GrantClaims,
    crypto::{Keypair, PublicKey},
};

#[allow(deprecated, reason = "Internal use of deprecated public API")]
use crate::{AuthToken, PubkyHttpClient, PubkySession, actors::Pkdns};
use crate::errors::{AuthError, Result};

/// Approval payload delivered through the relay channel.
///
/// The relay protocol carries opaque encrypted bytes; once decrypted, those
/// bytes can either be a postcard-encoded legacy [`AuthToken`] or a UTF-8
/// `pubky-grant` JWS string. We try the grant interpretation first and fall
/// back to the legacy [`AuthToken`] — relay payloads are tagged by content
/// shape, not by an explicit protocol version, so old apps still work
/// unchanged.
///
/// Both variants are boxed to keep the enum size small and uniform; the
/// legacy [`AuthToken`] carries a 64-byte signature, namespace, timestamp,
/// public key, and capabilities, which would otherwise dominate the variant
/// layout.
#[derive(Debug)]
pub(crate) enum AuthApproval {
    /// Legacy postcard-encoded [`AuthToken`] (cookie flow).
    Legacy(Box<AuthToken>),
    /// User-signed grant JWS (grant + JWT flow).
    Grant {
        jws: String,
        claims: Box<GrantClaims>,
    },
}

impl AuthApproval {
    /// Parse a relay payload into either a legacy [`AuthToken`] or a grant JWS.
    ///
    /// Tries the grant interpretation first (3-segment base64url JWS that
    /// decodes to [`GrantClaims`]). Falls back to verifying the bytes as a
    /// postcard-encoded [`AuthToken`].
    ///
    /// # Verification asymmetry
    /// - **Grant** payloads are decoded only — the homeserver verifies the
    ///   signature when the SDK posts the grant to `/auth/jwt/session`.
    /// - **Legacy** payloads go through [`AuthToken::verify`], which checks
    ///   the signature, namespace, and timestamp window here in the SDK.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the bytes are
    ///   neither a valid grant JWS nor a verifiable [`AuthToken`].
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        if let Ok(text) = std::str::from_utf8(bytes) {
            // Grant JWS Compact strings are 3 dot-separated base64url segments.
            // Decode-only here; the homeserver verifies the signature.
            if let Ok(claims) = GrantClaims::decode(text) {
                return Ok(Self::Grant {
                    jws: text.to_string(),
                    claims: Box::new(claims),
                });
            }
        }
        let token = AuthToken::verify(bytes)?;
        Ok(Self::Legacy(Box::new(token)))
    }
}

/// Context required to convert an [`AuthApproval`] into a JWT-backed
/// [`PubkySession`]. Captured at flow construction time.
#[derive(Clone)]
pub(crate) struct GrantContext {
    /// Client (`PoP`) keypair bound by the grant's `cnf` claim.
    pub client_keypair: Keypair,
    /// For sign-up flows: the homeserver to create the user on, plus an
    /// optional signup token. For sign-in flows this is `None` and the
    /// homeserver is resolved from PKARR after the grant arrives.
    pub signup_homeserver: Option<(PublicKey, Option<String>)>,
}

impl fmt::Debug for GrantContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrantContext")
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
    grant_ctx: Option<GrantContext>,
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
