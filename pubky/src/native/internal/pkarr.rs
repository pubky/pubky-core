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
    pub async fn get_homeserver(&self, pubky: &PublicKey) -> Option<String> {
        let packet = self.pkarr.resolve_most_recent(pubky).await;
        if packet.is_none() {
            return None;
        }

        // Check for the `_pubky` SVCB record.
        let packet = packet.unwrap();
        let name = format!("_pubky.{}", pubky.to_z32());
        let maching_names = packet.resource_records(name.as_str()).collect::<Vec<_>>();

        let pubky_records = maching_names.into_iter()
        .map(|r| r.rdata.clone())
        .filter(|r| matches!(r, RData::HTTPS(_))).collect::<Vec<_>>();

        if pubky_records.is_empty() {
            return None;
        }

        let record = pubky_records.first().unwrap();
        if let RData::HTTPS(svc) = record {
            return Some(svc.target.to_string());
        }
        None
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_homeserver() {
        let dht = mainline::Testnet::new(3).unwrap();
        let client = Client::builder().pkarr(|builder| {
            builder.bootstrap(&dht.bootstrap)
        }).build().unwrap();
        let keypair = Keypair::random();
        let pubky = keypair.public_key();

        let homeserver_key = Keypair::random().public_key().to_z32();
        client.publish_homeserver(&keypair, homeserver_key.as_str()).await.unwrap();
        let homeserver = client.get_homeserver(&pubky).await;
        assert_eq!(homeserver, Some(homeserver_key));
    }
}