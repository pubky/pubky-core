use reqwest::StatusCode;

pub use pkarr::{PublicKey, SignedPacket};

use crate::error::Result;
use crate::PubkyClient;

// TODO: Add an in memory cache of packets

impl PubkyClient {
    //TODO: migrate to pkarr::PkarrRelayClient
    pub(crate) async fn pkarr_resolve(
        &self,
        public_key: &PublicKey,
    ) -> Result<Option<SignedPacket>> {
        //TODO: Allow multiple relays in parallel
        let relay = self.pkarr_relays.first().expect("initialized with relays");

        let res = self
            .http
            .get(format!("{relay}/{}", public_key))
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
        let relay = self.pkarr_relays.first().expect("initialized with relays");

        self.http
            .put(format!("{relay}/{}", signed_packet.public_key()))
            .body(signed_packet.to_relay_payload())
            .send()
            .await?;

        Ok(())
    }
}
