#[allow(deprecated, reason = "Internal use of deprecated public API")]
use crate::AuthToken;
use crate::actors::auth::relay::AuthRelayMessage;
use crate::errors::Result;

/// Verified legacy auth token delivered through the relay channel.
#[allow(deprecated, reason = "Internal use of deprecated public API")]
#[derive(Debug)]
pub(crate) struct CookieApproval(pub(crate) AuthToken);

impl CookieApproval {
    /// Verify a relay message as a postcard-encoded [`AuthToken`].
    pub(crate) fn decode(message: &AuthRelayMessage) -> Result<Self> {
        #[allow(deprecated, reason = "Internal use of deprecated public API")]
        let token = AuthToken::verify(message.as_bytes())?;
        Ok(Self(token))
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::capabilities::Capabilities;

    use super::*;
    use crate::Keypair;

    #[test]
    fn decode_verifies_valid_legacy_token() {
        let keypair = Keypair::random();
        #[allow(deprecated, reason = "Internal use of deprecated public API")]
        let token = AuthToken::sign(&keypair, Capabilities::default());
        let message = AuthRelayMessage::new(token.serialize());

        let approval = CookieApproval::decode(&message).unwrap();

        assert_eq!(approval.0, token);
    }
}
