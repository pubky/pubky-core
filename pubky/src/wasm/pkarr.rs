use reqwest::StatusCode;

pub use pkarr::{PublicKey, SignedPacket};

use crate::error::Result;
use crate::PubkyClient;

const TEST_RELAY: &str = "http://localhost:15411/pkarr";

// TODO: Add an in memory cache of packets

impl PubkyClient {
    //TODO: Allow multiple relays in parallel
    //TODO: migrate to pkarr::PkarrRelayClient
    pub(crate) async fn pkarr_resolve(
        &self,
        public_key: &PublicKey,
    ) -> Result<Option<SignedPacket>> {
        let res = self
            .http
            .get(format!("{TEST_RELAY}/{}", public_key))
            .send()
            .await?;

        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        };

        // TODO: guard against too large responses.
        let bytes = res.bytes().await?;

        let existing = SignedPacket::from_relay_payload(public_key, &bytes)?;

        Ok(Some(existing))
    }

    pub(crate) async fn pkarr_publish(&self, signed_packet: &SignedPacket) -> Result<()> {
        self.http
            .put(format!("{TEST_RELAY}/{}", signed_packet.public_key()))
            .body(signed_packet.to_relay_payload())
            .send()
            .await?;

        Ok(())
    }
}
