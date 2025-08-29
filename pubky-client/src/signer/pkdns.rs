use std::time::Duration;

use pkarr::{
    PublicKey, SignedPacket, Timestamp,
    dns::rdata::{RData, SVCB},
};

use super::core::PubkySigner;
use crate::errors::{Error, PkarrError, Result};

/// Publish strategy.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PublishStrategy {
    Force,
    IfOlderThan,
}

impl PubkySigner {
    /// Publish `_pubky` record forcing a refresh.
    /// If `host_override` is `None`, reuse the host from the existing record (if any).
    pub async fn publish_homeserver_force(&self, host_override: Option<&PublicKey>) -> Result<()> {
        self.publish_homeserver(host_override, PublishStrategy::Force)
            .await
    }

    /// Publish `_pubky` record only if stale/missing.
    /// If `host_override` is `None`, reuse the host from the existing record (if any).
    pub async fn publish_homeserver_if_stale(
        &self,
        host_override: Option<&PublicKey>,
    ) -> Result<()> {
        self.publish_homeserver(host_override, PublishStrategy::IfOlderThan)
            .await
    }

    async fn publish_homeserver(
        &self,
        host_override: Option<&PublicKey>,
        mode: PublishStrategy,
    ) -> Result<()> {
        let pubky = self.pubky();

        // 1) Resolve existing record once.
        let existing = self.client.pkarr().resolve_most_recent(&pubky).await;

        // 2) Choose host to publish.
        let host_str = match determine_host(host_override, existing.as_ref()) {
            Some(h) => h,
            None => return Ok(()),
        };

        // 3) Age check for IfOlderThan.
        if matches!(mode, PublishStrategy::IfOlderThan) {
            if let Some(ref record) = existing {
                let elapsed = Timestamp::now() - record.timestamp();
                let age = Duration::from_micros(elapsed.as_u64());
                if age <= self.client.max_record_age() {
                    return Ok(());
                }
            }
        }

        // 4) Publish with small retry loop on retryable pkarr errors.
        for attempt in 1..=3 {
            match self
                .publish_homeserver_inner(&host_str, existing.clone())
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if let Error::Pkarr(pk) = &e {
                        if pk.is_retryable() && attempt < 3 {
                            continue;
                        }
                    }
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    async fn publish_homeserver_inner(
        &self,
        host: &str,
        existing: Option<SignedPacket>,
    ) -> Result<()> {
        // Keep previous records that are not `_pubky.*`, then write `_pubky` HTTPS/SVCB.
        let mut builder = SignedPacket::builder();
        if let Some(ref packet) = existing {
            for answer in packet.resource_records("_pubky") {
                if !answer.name.to_string().starts_with("_pubky") {
                    builder = builder.record(answer.to_owned());
                }
            }
        }

        let svcb = SVCB::new(0, host.try_into().map_err(PkarrError::from)?);
        let signed_packet = builder
            .https("_pubky".try_into().unwrap(), svcb, 60 * 60)
            // Sign with our keypair
            .sign(&self.keypair)
            .map_err(PkarrError::from)?;

        self.client
            .pkarr()
            .publish(&signed_packet, existing.map(|s| s.timestamp()))
            .await
            .map_err(PkarrError::from)?;

        Ok(())
    }
}

/// Extract `_pubky` SVCB/HTTPS target from a signed Pkarr packet.
fn extract_host_from_packet(packet: &SignedPacket) -> Option<String> {
    packet
        .resource_records("_pubky")
        .find_map(|rr| match &rr.rdata {
            RData::SVCB(svcb) => Some(svcb.target.to_string()),
            RData::HTTPS(https) => Some(https.0.target.to_string()),
            _ => None,
        })
}

fn determine_host(
    override_host: Option<&PublicKey>,
    dht_packet: Option<&SignedPacket>,
) -> Option<String> {
    if let Some(host) = override_host {
        return Some(host.to_string());
    }
    dht_packet.and_then(extract_host_from_packet)
}
