use pubky_common::auth::grant::GrantClaims;

#[allow(deprecated, reason = "Internal use of deprecated public API")]
use crate::AuthToken;
use crate::errors::{AuthError, Result};

/// Decrypted auth message delivered through the relay channel.
#[derive(Debug, Clone)]
pub(crate) struct AuthRelayMessage(Vec<u8>);

impl AuthRelayMessage {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Which auth payload shape this flow expects from the signer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthApprovalMode {
    LegacyToken,
    GrantJwt,
}

/// Approval payload delivered through the relay channel.
///
/// The relay protocol carries opaque encrypted bytes. The auth flow already
/// knows which payload shape it expects, so decoding is explicit:
/// legacy flows verify a postcard-encoded [`AuthToken`], while grant flows
/// decode a UTF-8 `pubky-grant` JWS.
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
    fn decode_legacy(message: &AuthRelayMessage) -> Result<Self> {
        let token = AuthToken::verify(message.as_bytes())?;
        Ok(Self::Legacy(Box::new(token)))
    }

    fn decode_grant(message: &AuthRelayMessage) -> Result<Self> {
        let text = std::str::from_utf8(message.as_bytes())
            .map_err(|e| AuthError::Validation(format!("invalid grant payload encoding: {e}")))?;
        let claims = GrantClaims::decode(text)
            .map_err(|e| AuthError::Validation(format!("invalid grant payload: {e}")))?;

        Ok(Self::Grant {
            jws: text.to_string(),
            claims: Box::new(claims),
        })
    }

    /// Decode a relay message into the auth payload shape expected by the flow.
    pub(crate) fn decode(
        message: &AuthRelayMessage,
        mode: AuthApprovalMode,
    ) -> Result<AuthApproval> {
        match mode {
            AuthApprovalMode::LegacyToken => Self::decode_legacy(message),
            AuthApprovalMode::GrantJwt => Self::decode_grant(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::{
        auth::jws::{ClientId, GrantId, sign_jws},
        capabilities::Capabilities,
    };

    use super::*;
    use crate::Keypair;

    #[test]
    fn decode_legacy_message_in_legacy_mode() {
        let keypair = Keypair::random();
        let token = AuthToken::sign(&keypair, Capabilities::default());
        let message = AuthRelayMessage::new(token.serialize());

        let approval = AuthApproval::decode(&message, AuthApprovalMode::LegacyToken).unwrap();

        match approval {
            AuthApproval::Legacy(decoded) => assert_eq!(*decoded, token),
            AuthApproval::Grant { .. } => panic!("expected legacy approval"),
        }
    }

    #[test]
    fn decode_legacy_message_in_grant_mode_fails() {
        let keypair = Keypair::random();
        let token = AuthToken::sign(&keypair, Capabilities::default());
        let message = AuthRelayMessage::new(token.serialize());

        let error = AuthApproval::decode(&message, AuthApprovalMode::GrantJwt).unwrap_err();

        assert!(error.to_string().contains("invalid grant payload"));
    }

    #[test]
    fn decode_grant_message_in_grant_mode() {
        let user_keypair = Keypair::random();
        let client_keypair = Keypair::random();
        let claims = GrantClaims {
            iss: user_keypair.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: Capabilities::default().0,
            cnf: client_keypair.public_key(),
            jti: GrantId::generate(),
            iat: 1,
            exp: 2,
        };
        let grant_jws = sign_jws(&user_keypair, "pubky-grant", &claims);
        let message = AuthRelayMessage::new(grant_jws.clone().into_bytes());

        let approval = AuthApproval::decode(&message, AuthApprovalMode::GrantJwt).unwrap();

        match approval {
            AuthApproval::Grant {
                jws,
                claims: decoded,
            } => {
                assert_eq!(jws, grant_jws);
                assert_eq!(*decoded, claims);
            }
            AuthApproval::Legacy(_) => panic!("expected grant approval"),
        }
    }

    #[test]
    fn decode_grant_message_in_legacy_mode_fails() {
        let user_keypair = Keypair::random();
        let claims = GrantClaims {
            iss: user_keypair.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: Capabilities::default().0,
            cnf: Keypair::random().public_key(),
            jti: GrantId::generate(),
            iat: 1,
            exp: 2,
        };
        let grant_jws = sign_jws(&user_keypair, "pubky-grant", &claims);
        let message = AuthRelayMessage::new(grant_jws.into_bytes());

        assert!(AuthApproval::decode(&message, AuthApprovalMode::LegacyToken).is_err());
    }
}
