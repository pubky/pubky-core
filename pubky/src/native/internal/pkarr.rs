use pkarr::{dns::rdata::SVCB, Keypair, SignedPacket};

use anyhow::Result;

use super::super::Client;

impl Client {
    /// Publish the HTTPS record for `_pubky.<public_key>`.
    pub(crate) async fn publish_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        // TODO: Before making public, consider the effect on other records and other mirrors

        let existing = self.pkarr.resolve_most_recent(&keypair.public_key()).await;

        let mut signed_packet_builder = SignedPacket::builder();

        if let Some(ref existing) = existing {
            for answer in existing.resource_records("_pubky") {
                if !answer.name.to_string().starts_with("_pubky") {
                    signed_packet_builder = signed_packet_builder.record(answer.to_owned());
                }
            }
        }

        let svcb = SVCB::new(0, host.try_into()?);

        let signed_packet = SignedPacket::builder()
            .https("_pubky".try_into().unwrap(), svcb, 60 * 60)
            .sign(keypair)?;

        self.pkarr
            .publish(&signed_packet, existing.map(|s| s.timestamp()))
            .await?;

        Ok(())
    }
}
