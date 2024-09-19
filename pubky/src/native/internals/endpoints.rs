use std::net::ToSocketAddrs;

use pkarr::dns::rdata::{RData, SVCB};
use pkarr::{PkarrClientAsync, PublicKey, SignedPacket};
use reqwest::dns::{Addrs, Resolve};

use crate::error::{Error, Result};

const MAX_ENDPOINT_RESOLUTION_RECURSION: u8 = 3;

#[derive(Debug, Clone)]
pub struct PkarrResolver {
    pkarr: PkarrClientAsync,
}

impl PkarrResolver {
    pub fn new(pkarr: PkarrClientAsync) -> Self {
        Self { pkarr }
    }

    /// Resolve a `qname` to an alternative [Endpoint] as defined in [RFC9460](https://www.rfc-editor.org/rfc/rfc9460#name-terminology).
    ///
    /// A `qname` is can be either a regular domain name for HTTPS endpoints,
    /// or it could use Attrleaf naming pattern for cusotm protcol. For example:
    /// `_foo.example.com` for `foo://example.com`.
    async fn resolve_endpoint(&self, qname: &str) -> Result<Endpoint> {
        let target = qname;
        // TODO: cache the result of this function?

        let mut step = 0;
        let mut svcb: Option<Endpoint> = None;

        loop {
            let current = svcb.clone().map_or(target.to_string(), |s| s.target);
            if let Ok(tld) = PublicKey::try_from(current.clone()) {
                if let Ok(Some(signed_packet)) = self.pkarr.resolve(&tld).await {
                    if step >= MAX_ENDPOINT_RESOLUTION_RECURSION {
                        break;
                    };
                    step += 1;

                    // Choose most prior SVCB record
                    svcb = get_endpoint(&signed_packet, &current);

                    // TODO: support wildcard?

                    if step >= MAX_ENDPOINT_RESOLUTION_RECURSION {
                        break;
                    };
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if let Some(svcb) = svcb {
            if PublicKey::try_from(svcb.target.as_str()).is_err() {
                return Ok(svcb);
            }
        }

        Err(Error::ResolveEndpoint(target.into()))
    }
}

impl Resolve for PkarrResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let client = self.clone();

        Box::pin(async move {
            let name = name.as_str();

            if PublicKey::try_from(name).is_ok() {
                let x = client.resolve_endpoint(name).await?;

                let addrs = format!("{}:{}", x.target, x.port).to_socket_addrs()?;

                let addrs: Addrs = Box::new(addrs);

                return Ok(addrs);
            };

            Ok(Box::new(format!("{name}:0").to_socket_addrs().unwrap()))
        })
    }
}

#[derive(Debug, Clone)]
struct Endpoint {
    target: String,
    // public_key: PublicKey,
    port: u16,
}

fn get_endpoint(signed_packet: &SignedPacket, target: &str) -> Option<Endpoint> {
    let is_svcb = target.starts_with('_');

    signed_packet
        .resource_records(target)
        .fold(None, |prev: Option<SVCB>, answer| {
            if let Some(svcb) = match &answer.rdata {
                RData::SVCB(svcb) => {
                    if is_svcb {
                        Some(svcb)
                    } else {
                        None
                    }
                }
                RData::HTTPS(curr) => {
                    if is_svcb {
                        None
                    } else {
                        Some(&curr.0)
                    }
                }
                _ => None,
            } {
                let curr = svcb.clone();

                if curr.priority == 0 {
                    return Some(curr);
                }
                if let Some(prev) = &prev {
                    if curr.priority >= prev.priority {
                        return Some(curr);
                    }
                } else {
                    return Some(curr);
                }
            }

            prev
        })
        .map(|s| Endpoint {
            target: s.target.to_string(),
            // public_key: signed_packet.public_key(),
            port: u16::from_be_bytes(
                s.get_param(SVCB::PORT)
                    .unwrap_or_default()
                    .try_into()
                    .unwrap_or([0, 0]),
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::dns::{self, rdata::RData};
    use pkarr::PkarrClient;
    use pkarr::{mainline::Testnet, Keypair};

    async fn publish_packets(
        client: &PkarrClientAsync,
        tree: Vec<Vec<(&str, RData<'static>)>>,
    ) -> Vec<Keypair> {
        let mut keypairs: Vec<Keypair> = Vec::with_capacity(tree.len());
        for node in tree {
            let mut packet = dns::Packet::new_reply(0);
            for record in node {
                packet.answers.push(dns::ResourceRecord::new(
                    dns::Name::new(record.0).unwrap(),
                    dns::CLASS::IN,
                    3600,
                    record.1,
                ));
            }
            let keypair = Keypair::random();
            let signed_packet = SignedPacket::from_packet(&keypair, &packet).unwrap();
            keypairs.push(keypair);
            client.publish(&signed_packet).await.unwrap();
        }
        keypairs
    }

    #[tokio::test]
    async fn resolve_direct_endpoint() {
        let testnet = Testnet::new(3);
        let pkarr = PkarrClient::builder()
            .testnet(&testnet)
            .build()
            .unwrap()
            .as_async();

        let keypairs = publish_packets(
            &pkarr,
            vec![vec![
                (
                    "foo",
                    RData::HTTPS(SVCB::new(0, "https.example.com".try_into().unwrap()).into()),
                ),
                // Make sure HTTPS only follows HTTPs
                (
                    "foo",
                    RData::SVCB(SVCB::new(0, "protocol.example.com".try_into().unwrap())),
                ),
                // Make sure SVCB only follows SVCB
                (
                    "foo",
                    RData::HTTPS(SVCB::new(0, "https.example.com".try_into().unwrap()).into()),
                ),
                (
                    "_foo",
                    RData::SVCB(SVCB::new(0, "protocol.example.com".try_into().unwrap())),
                ),
            ]],
        )
        .await;

        let resolver = PkarrResolver { pkarr };

        let tld = keypairs.first().unwrap().public_key();

        // Follow foo.tld HTTPS records
        let endpoint = resolver
            .resolve_endpoint(&format!("foo.{tld}"))
            .await
            .unwrap();
        assert_eq!(endpoint.target, "https.example.com");

        // Follow _foo.tld SVCB records
        let endpoint = resolver
            .resolve_endpoint(&format!("_foo.{tld}"))
            .await
            .unwrap();
        assert_eq!(endpoint.target, "protocol.example.com");
    }
}
