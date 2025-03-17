use anyhow::Result;
use pkarr::{
    dns::rdata::{RData, SVCB},
    Keypair, SignedPacket, Timestamp,
};
use std::convert::TryInto;
use std::time::Duration;

use super::super::Client;

/// The strategy to decide whether to (re)publish a homeserver record.
pub(crate) enum PublishStrategy {
    /// Always publish a new record (used on signup).
    Force,
    /// Only publish if no record can be resolved or if the record is older than 1 hour.
    /// Used on signin and on republish_homeserver (used by key managing apps)
    IfOlderThan,
}

impl Client {
    /// Unified method to update the homeserver record.
    ///
    /// If `host` is provided, that value is used; otherwise the host is extracted from the
    /// currently resolved record. Under the IfOlderThan strategy, the record is only updated if
    /// it is missing or its timestamp is older than 1 hour. Under the Force strategy, the
    /// record is always published.
    pub(crate) async fn publish_homeserver(
        &self,
        keypair: &Keypair,
        host: Option<&str>,
        strategy: PublishStrategy,
    ) -> Result<()> {
        // Resolve the most recent record.
        let existing = self.pkarr.resolve_most_recent(&keypair.public_key()).await;

        // Determine which host we should be using.
        let host_str = match Self::determine_host(host, existing.as_ref()) {
            Some(host) => host,
            None => return Ok(()),
        };

        // Determine if we should publish based on the given strategy.
        let should_publish = match strategy {
            PublishStrategy::Force => true,
            PublishStrategy::IfOlderThan => match existing {
                Some(ref record) => {
                    let elapsed = Timestamp::now() - record.timestamp();
                    Duration::from_micros(elapsed.as_u64()) > self.max_record_age
                }
                None => true,
            },
        };

        if should_publish {
            self.publish_homeserver_inner(keypair, &host_str, existing)
                .await?;
        }

        Ok(())
    }

    /// Internal helper that builds and publishes the PKarr record.
    /// Uses an optionally pre-resolved record to avoid re-resolving.
    async fn publish_homeserver_inner(
        &self,
        keypair: &Keypair,
        host: &str,
        existing: Option<SignedPacket>,
    ) -> Result<()> {
        let mut builder = SignedPacket::builder();
        if let Some(ref packet) = existing {
            // Append any records (except those already starting with "_pubky") to our builder.
            for answer in packet.resource_records("_pubky") {
                if !answer.name.to_string().starts_with("_pubky") {
                    builder = builder.record(answer.to_owned());
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

    /// Helper determines the host to publish, prioritizing an explicit
    /// override or extracting from an existing DHT packet. Returns `None`
    /// if neither source provides a host.
    fn determine_host(
        override_host: Option<&str>,
        dht_packet: Option<&SignedPacket>,
    ) -> Option<String> {
        if let Some(host) = override_host {
            return Some(host.to_string());
        }
        dht_packet.and_then(Self::extract_host_from_packet)
    }

    /// Helper to extract the current homeserver host from a signed PKarr packet.
    /// Iterates over the records with name "_pubky" and returns the first SVCB target found.
    fn extract_host_from_packet(packet: &SignedPacket) -> Option<String> {
        packet
            .resource_records("_pubky")
            .find_map(|rr| match &rr.rdata {
                RData::SVCB(svcb) => Some(svcb.target.to_string()),
                RData::HTTPS(https) => Some(https.0.target.to_string()),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Client;
    use pkarr::dns::rdata::SVCB;
    use pkarr::Keypair;

    #[tokio::test]
    async fn test_extract_host_from_packet() -> Result<()> {
        let keypair = Keypair::random();
        // Define the host that we want to encode.
        let host = "host.example.com";
        // Create an SVCB record with that host.
        let svcb = SVCB::new(0, host.try_into()?);
        // Build a signed packet containing an HTTPS record for "_pubky".
        let signed_packet = SignedPacket::builder()
            .https("_pubky".try_into().unwrap(), svcb, 60 * 60)
            .sign(&keypair)?;
        // Use our helper to extract the host.
        let extracted_host = Client::extract_host_from_packet(&signed_packet);
        // Verify that the extracted host matches what we set.
        assert_eq!(extracted_host.as_deref(), Some(host));
        Ok(())
    }
}
