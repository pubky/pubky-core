use std::net::ToSocketAddrs;

use pkarr::{PkarrClientAsync, PublicKey};
use reqwest::dns::{Addrs, Resolve};

use crate::error::{Error, Result};

use super::endpoints::Endpoint;

const DEFAULT_MAX_CHAIN_LENGTH: u8 = 3;

#[derive(Debug, Clone)]
pub struct PkarrResolver {
    pkarr: PkarrClientAsync,
    max_chain_length: u8,
}

impl PkarrResolver {
    pub fn new(pkarr: PkarrClientAsync, max_chain_length: u8) -> Self {
        PkarrResolver {
            pkarr,
            max_chain_length,
        }
    }

    /// Resolve a `qname` to an alternative [Endpoint] as defined in [RFC9460](https://www.rfc-editor.org/rfc/rfc9460#name-terminology).
    ///
    /// A `qname` is can be either a regular domain name for HTTPS endpoints,
    /// or it could use Attrleaf naming pattern for cusotm protcol. For example:
    /// `_foo.example.com` for `foo://example.com`.
    async fn resolve_endpoint(&self, qname: &str) -> Result<Endpoint> {
        let target = qname;
        // TODO: cache the result of this function?

        let is_svcb = target.starts_with('_');

        let mut step = 0;
        let mut svcb: Option<Endpoint> = None;

        loop {
            let current = svcb.clone().map_or(target.to_string(), |s| s.target);
            if let Ok(tld) = PublicKey::try_from(current.clone()) {
                if let Ok(Some(signed_packet)) = self.pkarr.resolve(&tld).await {
                    if step >= self.max_chain_length {
                        break;
                    };
                    step += 1;

                    // Choose most prior SVCB record
                    svcb = Endpoint::find(&signed_packet, &current, is_svcb);

                    // TODO: support wildcard?
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
                let endpoint = client.resolve_endpoint(name).await?;

                // let addrs = format!("{}:{}", x.target, x.port).to_socket_addrs()?;

                let addrs: Addrs = Box::new(endpoint.to_socket_addrs()?);

                return Ok(addrs);
            };

            Ok(Box::new(format!("{name}:0").to_socket_addrs().unwrap()))
        })
    }
}

impl From<&PkarrClientAsync> for PkarrResolver {
    fn from(pkarr: &PkarrClientAsync) -> Self {
        pkarr.clone().into()
    }
}

impl From<PkarrClientAsync> for PkarrResolver {
    fn from(pkarr: PkarrClientAsync) -> Self {
        Self::new(pkarr, DEFAULT_MAX_CHAIN_LENGTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::dns::rdata::{A, SVCB};
    use pkarr::dns::{self, rdata::RData};
    use pkarr::{mainline::Testnet, Keypair};
    use pkarr::{PkarrClient, SignedPacket};

    use std::future::Future;
    use std::pin::Pin;

    fn generate_subtree(
        client: PkarrClientAsync,
        depth: u8,
        branching: u8,
        domain: Option<String>,
    ) -> Pin<Box<dyn Future<Output = PublicKey>>> {
        Box::pin(async move {
            let keypair = Keypair::random();

            let mut packet = dns::Packet::new_reply(0);

            for _ in 0..branching {
                let mut svcb = SVCB::new(0, ".".try_into().unwrap());

                if depth == 0 {
                    svcb.priority = 1;
                    svcb.set_port((branching) as u16 * 1000);

                    if let Some(target) = &domain {
                        let target: &'static str = Box::leak(target.clone().into_boxed_str());
                        svcb.target = target.try_into().unwrap()
                    }
                } else {
                    let target =
                        generate_subtree(client.clone(), depth - 1, branching, domain.clone())
                            .await
                            .to_string();
                    let target: &'static str = Box::leak(target.into_boxed_str());
                    svcb.target = target.try_into().unwrap();
                };

                packet.answers.push(dns::ResourceRecord::new(
                    dns::Name::new("@").unwrap(),
                    dns::CLASS::IN,
                    3600,
                    RData::HTTPS(svcb.into()),
                ));
            }

            if depth == 0 {
                packet.answers.push(dns::ResourceRecord::new(
                    dns::Name::new("@").unwrap(),
                    dns::CLASS::IN,
                    3600,
                    RData::A(A { address: 10 }),
                ));
            }

            let signed_packet = SignedPacket::from_packet(&keypair, &packet).unwrap();
            client.publish(&signed_packet).await.unwrap();

            keypair.public_key()
        })
    }

    fn generate(
        client: PkarrClientAsync,
        depth: u8,
        branching: u8,
        domain: Option<String>,
    ) -> Pin<Box<dyn Future<Output = PublicKey>>> {
        generate_subtree(client, depth - 1, branching, domain)
    }

    #[tokio::test]
    async fn resolve_endpoints() {
        let testnet = Testnet::new(3);
        let pkarr = PkarrClient::builder()
            .testnet(&testnet)
            .build()
            .unwrap()
            .as_async();

        let resolver: PkarrResolver = (&pkarr).into();
        let tld = generate(pkarr, 3, 3, Some("example.com".to_string())).await;

        let endpoint = resolver.resolve_endpoint(&tld.to_string()).await.unwrap();
        assert_eq!(endpoint.target, "example.com");
    }

    #[tokio::test]
    async fn max_chain_exceeded() {
        let testnet = Testnet::new(3);
        let pkarr = PkarrClient::builder()
            .testnet(&testnet)
            .build()
            .unwrap()
            .as_async();

        let resolver: PkarrResolver = (&pkarr).into();

        let tld = generate(pkarr, 4, 3, Some("example.com".to_string())).await;

        let endpoint = resolver.resolve_endpoint(&tld.to_string()).await;

        assert_eq!(
            match endpoint {
                Err(error) => error.to_string(),
                _ => "".to_string(),
            },
            crate::Error::ResolveEndpoint(tld.to_string()).to_string()
        )
    }

    #[tokio::test]
    async fn resolve_addresses() {
        let testnet = Testnet::new(3);
        let pkarr = PkarrClient::builder()
            .testnet(&testnet)
            .build()
            .unwrap()
            .as_async();

        let resolver: PkarrResolver = (&pkarr).into();
        let tld = generate(pkarr, 3, 3, None).await;

        let endpoint = resolver.resolve_endpoint(&tld.to_string()).await.unwrap();
        assert_eq!(endpoint.target, ".");
        assert_eq!(endpoint.port, 3000);
        assert_eq!(
            endpoint
                .to_socket_addrs()
                .unwrap()
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
            vec!["0.0.0.10:3000"]
        );
        dbg!(&endpoint);
    }
}
