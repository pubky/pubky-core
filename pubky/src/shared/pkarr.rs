use url::Url;

use pkarr::{
    dns::{rdata::SVCB, Packet},
    Keypair, PublicKey, SignedPacket,
};

use crate::{
    error::{Error, Result},
    PubkyClient,
};

const MAX_ENDPOINT_RESOLUTION_RECURSION: u8 = 3;

impl PubkyClient {
    /// Publish the SVCB record for `_pubky.<public_key>`.
    pub(crate) async fn publish_pubky_homeserver(
        &self,
        keypair: &Keypair,
        host: &str,
    ) -> Result<()> {
        let existing = self.pkarr_resolve(&keypair.public_key()).await?;

        let mut packet = Packet::new_reply(0);

        if let Some(existing) = existing {
            for answer in existing.packet().answers.iter().cloned() {
                if !answer.name.to_string().starts_with("_pubky") {
                    packet.answers.push(answer.into_owned())
                }
            }
        }

        let svcb = SVCB::new(0, host.try_into()?);

        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "_pubky".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            60 * 60,
            pkarr::dns::rdata::RData::SVCB(svcb),
        ));

        let signed_packet = SignedPacket::from_packet(keypair, &packet)?;

        self.pkarr_publish(&signed_packet).await?;

        Ok(())
    }

    /// Resolve the homeserver for a pubky.
    pub(crate) async fn resolve_pubky_homeserver(&self, pubky: &PublicKey) -> Result<Endpoint> {
        let target = format!("_pubky.{pubky}");

        self.resolve_endpoint(&target)
            .await
            .map_err(|_| Error::Generic("Could not resolve homeserver".to_string()))
    }

    /// Resolve a service's public_key and "non-pkarr url" from a Pubky domain
    ///
    /// "non-pkarr" url is any URL where the hostname isn't a 52 z-base32 character,
    /// usually an IPv4, IPv6 or ICANN domain, but could also be any other unknown hostname.
    ///
    /// Recursively resolve SVCB and HTTPS endpoints, with [MAX_ENDPOINT_RESOLUTION_RECURSION] limit.
    pub(crate) async fn resolve_endpoint(&self, target: &str) -> Result<Endpoint> {
        let original_target = target;
        // TODO: cache the result of this function?

        let mut target = target.to_string();

        let mut endpoint_public_key = None;
        let mut origin = target.clone();

        let mut step = 0;

        // PublicKey is very good at extracting the Pkarr TLD from a string.
        while let Ok(public_key) = PublicKey::try_from(target.clone()) {
            if step >= MAX_ENDPOINT_RESOLUTION_RECURSION {
                break;
            };
            step += 1;

            if let Some(signed_packet) = self
                .pkarr_resolve(&public_key)
                .await
                .map_err(|_| Error::ResolveEndpoint(original_target.into()))?
            {
                // Choose most prior SVCB record
                let svcb = signed_packet.resource_records(&target).fold(
                    None,
                    |prev: Option<SVCB>, answer| {
                        if let Some(svcb) = match &answer.rdata {
                            pkarr::dns::rdata::RData::SVCB(svcb) => Some(svcb),
                            pkarr::dns::rdata::RData::HTTPS(curr) => Some(&curr.0),
                            _ => None,
                        } {
                            let curr = svcb.clone();

                            if curr.priority == 0 {
                                return Some(curr);
                            }
                            if let Some(prev) = &prev {
                                // TODO return random if priority is the same
                                if curr.priority >= prev.priority {
                                    return Some(curr);
                                }
                            } else {
                                return Some(curr);
                            }
                        }

                        prev
                    },
                );

                if let Some(svcb) = svcb {
                    endpoint_public_key = Some(public_key.clone());
                    target = svcb.target.to_string();

                    if let Some(port) = svcb.get_param(pkarr::dns::rdata::SVCB::PORT) {
                        if port.len() < 2 {
                            // TODO: debug! Error encoding port!
                        }
                        let port = u16::from_be_bytes([port[0], port[1]]);

                        origin = format!("{target}:{port}");
                    } else {
                        origin.clone_from(&target);
                    };

                    if step >= MAX_ENDPOINT_RESOLUTION_RECURSION {
                        continue;
                    };
                }
            } else {
                break;
            }
        }

        if PublicKey::try_from(origin.as_str()).is_ok() {
            return Err(Error::ResolveEndpoint(original_target.into()));
        }

        if endpoint_public_key.is_some() {
            let url = Url::parse(&format!(
                "{}://{}",
                if origin.starts_with("localhost") {
                    "http"
                } else {
                    "https"
                },
                origin
            ))?;

            return Ok(Endpoint { url });
        }

        Err(Error::ResolveEndpoint(original_target.into()))
    }

    pub(crate) async fn resolve_url(&self, url: &mut Url) -> Result<()> {
        if let Some(Ok(pubky)) = url.host_str().map(PublicKey::try_from) {
            let Endpoint { url: x, .. } = self.resolve_endpoint(&format!("_pubky.{pubky}")).await?;

            url.set_host(x.host_str())?;
            url.set_port(x.port()).expect("should work!");
            url.set_scheme(x.scheme()).expect("should work!");
        };

        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Endpoint {
    pub url: Url,
}

#[cfg(test)]
mod tests {
    use super::*;

    use pkarr::{
        dns::{
            rdata::{HTTPS, SVCB},
            Packet,
        },
        mainline::{dht::DhtSettings, Testnet},
        Keypair, PkarrClient, Settings, SignedPacket,
    };
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn resolve_endpoint_https() {
        let testnet = Testnet::new(10);

        let pkarr_client = PkarrClient::new(Settings {
            dht: DhtSettings {
                bootstrap: Some(testnet.bootstrap.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap()
        .as_async();

        let domain = "example.com";
        let mut target;

        // Server
        {
            let keypair = Keypair::random();

            let https = HTTPS(SVCB::new(0, domain.try_into().unwrap()));

            let mut packet = Packet::new_reply(0);

            packet.answers.push(pkarr::dns::ResourceRecord::new(
                "foo".try_into().unwrap(),
                pkarr::dns::CLASS::IN,
                60 * 60,
                pkarr::dns::rdata::RData::HTTPS(https),
            ));

            let signed_packet = SignedPacket::from_packet(&keypair, &packet).unwrap();

            pkarr_client.publish(&signed_packet).await.unwrap();

            target = format!("foo.{}", keypair.public_key());
        }

        // intermediate
        {
            let keypair = Keypair::random();

            let svcb = SVCB::new(0, target.as_str().try_into().unwrap());

            let mut packet = Packet::new_reply(0);

            packet.answers.push(pkarr::dns::ResourceRecord::new(
                "bar".try_into().unwrap(),
                pkarr::dns::CLASS::IN,
                60 * 60,
                pkarr::dns::rdata::RData::SVCB(svcb),
            ));

            let signed_packet = SignedPacket::from_packet(&keypair, &packet).unwrap();

            pkarr_client.publish(&signed_packet).await.unwrap();

            target = format!("bar.{}", keypair.public_key())
        }

        {
            let keypair = Keypair::random();

            let svcb = SVCB::new(0, target.as_str().try_into().unwrap());

            let mut packet = Packet::new_reply(0);

            packet.answers.push(pkarr::dns::ResourceRecord::new(
                "pubky".try_into().unwrap(),
                pkarr::dns::CLASS::IN,
                60 * 60,
                pkarr::dns::rdata::RData::SVCB(svcb),
            ));

            let signed_packet = SignedPacket::from_packet(&keypair, &packet).unwrap();

            pkarr_client.publish(&signed_packet).await.unwrap();

            target = format!("pubky.{}", keypair.public_key())
        }

        let client = PubkyClient::test(&testnet);

        let endpoint = client.resolve_endpoint(&target).await.unwrap();

        assert_eq!(endpoint.url.host_str().unwrap(), domain);
    }

    #[tokio::test]
    async fn resolve_homeserver() {
        let testnet = Testnet::new(10);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        // Publish an intermediate controller of the homeserver
        let pkarr_client = PkarrClient::new(Settings {
            dht: DhtSettings {
                bootstrap: Some(testnet.bootstrap.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap()
        .as_async();

        let intermediate = Keypair::random();

        let mut packet = Packet::new_reply(0);

        let server_tld = server.public_key().to_string();

        let svcb = SVCB::new(0, server_tld.as_str().try_into().unwrap());

        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "pubky".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            60 * 60,
            pkarr::dns::rdata::RData::SVCB(svcb),
        ));

        let signed_packet = SignedPacket::from_packet(&intermediate, &packet).unwrap();

        pkarr_client.publish(&signed_packet).await.unwrap();

        {
            let client = PubkyClient::test(&testnet);

            let pubky = Keypair::random();

            client
                .publish_pubky_homeserver(&pubky, &format!("pubky.{}", &intermediate.public_key()))
                .await
                .unwrap();

            let Endpoint { url, .. } = client
                .resolve_pubky_homeserver(&pubky.public_key())
                .await
                .unwrap();

            assert_eq!(url.host_str(), Some("localhost"));
            assert_eq!(url.port(), Some(server.port()));
        }
    }
}
