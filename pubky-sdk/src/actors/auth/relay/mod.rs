pub mod auth_relay_listener;
pub mod http_relay_inbox_channel;
pub mod http_relay_link_channel;

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
