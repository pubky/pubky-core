use pubky_common::auth::grant::GrantClaims;

use crate::actors::auth::relay::AuthRelayMessage;
use crate::errors::{AuthError, Result};

/// Verified `pubky-grant` JWS delivered through the relay channel.
#[derive(Debug)]
pub(crate) struct GrantApproval {
    pub(crate) jws: String,
    pub(crate) claims: GrantClaims,
}

impl GrantApproval {
    /// Decode a relay message as a UTF-8 `pubky-grant` JWS.
    pub(crate) fn decode(message: &AuthRelayMessage) -> Result<Self> {
        let text = std::str::from_utf8(message.as_bytes())
            .map_err(|e| AuthError::Validation(format!("invalid grant payload encoding: {e}")))?;
        let claims = GrantClaims::decode(text)
            .map_err(|e| AuthError::Validation(format!("invalid grant payload: {e}")))?;
        Ok(Self {
            jws: text.to_string(),
            claims,
        })
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
    fn decode_verifies_valid_grant() {
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

        let approval = GrantApproval::decode(&message).unwrap();

        assert_eq!(approval.jws, grant_jws);
        assert_eq!(approval.claims, claims);
    }
}
