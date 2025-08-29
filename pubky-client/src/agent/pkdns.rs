use pkarr::{PublicKey, SignedPacket, dns::rdata::RData};

use super::core::PubkyAgent;

/// Agent-scoped PKDNS (Pkarr) view.
#[derive(Debug, Clone, Copy)]
pub struct Pkdns<'a>(&'a PubkyAgent);

impl PubkyAgent {
    pub fn pkdns(&self) -> Pkdns<'_> {
        Pkdns(self)
    }
}

impl<'a> Pkdns<'a> {
    /// Resolve current homeserver host for any pubky via Pkarr.
    pub async fn get_homeserver(&self, pubky: &PublicKey) -> Option<String> {
        let packet = self.0.client.pkarr().resolve_most_recent(pubky).await?;
        extract_host_from_packet(&packet)
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
