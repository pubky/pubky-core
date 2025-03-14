use pkarr::{dns::rdata::{RData, SVCB}, Keypair, PublicKey, SignedPacket};

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

    /// Get the homeserver for a given Pubky public key.
    /// Looks up the pkarr packet for the given public key and returns the content of the first `_pubky` SVCB record.
    pub(crate) async fn get_homeserver(&self, pubky: &PublicKey) -> Option<String> {
        let packet = self.pkarr.resolve_most_recent(pubky).await;
        if packet.is_none() {
            return None;
        }

        // Check for the `_pubky` SVCB record.
        let packet = packet.unwrap();
        let pubky_records = packet.resource_records("_pubky")
        .map(|r| r.rdata.clone())
        .filter(|r| matches!(r, RData::SVCB(_))).collect::<Vec<_>>();
        if pubky_records.is_empty() {
            return None;
        }

        let record = pubky_records.first().unwrap();
        if let RData::SVCB(svc) = record {
            return Some(svc.target.to_string());
        }
        None
    }
}
