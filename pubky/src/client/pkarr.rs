pub use pkarr::{
    dns::{rdata::SVCB, Packet},
    mainline::{dht::DhtSettings, Testnet},
    Keypair, PkarrClient, PublicKey, Settings, SignedPacket,
};

use super::{Error, PubkyClient, Result, Url};

const MAX_RECURSIVE_PUBKY_HOMESERVER_RESOLUTION: u8 = 3;

impl PubkyClient {
    /// Publish the SVCB record for `_pubky.<public_key>`.
    pub(crate) fn publish_pubky_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        let mut packet = Packet::new_reply(0);

        if let Some(existing) = self.pkarr.resolve(&keypair.public_key())? {
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

        self.pkarr.publish(&signed_packet)?;

        Ok(())
    }

    /// Resolve the homeserver for a pubky.
    pub(crate) fn resolve_pubky_homeserver(&self, pubky: &PublicKey) -> Result<(PublicKey, Url)> {
        let target = format!("_pubky.{}", pubky);

        self.resolve_endpoint(&target)
            .map_err(|_| Error::Generic("Could not resolve homeserver".to_string()))
    }

    /// Resolve a service's public_key and clearnet url from a Pubky domain
    pub(crate) fn resolve_endpoint(&self, target: &str) -> Result<(PublicKey, Url)> {
        // TODO: cache the result of this function?
        // TODO: use MAX_RECURSIVE_PUBKY_HOMESERVER_RESOLUTION
        // TODO: move to common?

        let mut target = target.to_string();
        let mut homeserver_public_key = None;
        let mut host = target.clone();

        // PublicKey is very good at extracting the Pkarr TLD from a string.
        while let Ok(public_key) = PublicKey::try_from(target.clone()) {
            if let Some(signed_packet) = self.pkarr.resolve(&public_key)? {
                let mut prior = None;

                for answer in signed_packet.resource_records(&target) {
                    if let pkarr::dns::rdata::RData::SVCB(svcb) = &answer.rdata {
                        if svcb.priority == 0 {
                            prior = Some(svcb)
                        } else if let Some(sofar) = prior {
                            if svcb.priority >= sofar.priority {
                                prior = Some(svcb)
                            }
                            // TODO return random if priority is the same
                        } else {
                            prior = Some(svcb)
                        }
                    }
                }

                if let Some(svcb) = prior {
                    homeserver_public_key = Some(public_key);
                    target = svcb.target.to_string();

                    if let Some(port) = svcb.get_param(pkarr::dns::rdata::SVCB::PORT) {
                        if port.len() < 2 {
                            // TODO: debug! Error encoding port!
                        }
                        let port = u16::from_be_bytes([port[0], port[1]]);

                        host = format!("{target}:{port}");
                    } else {
                        host.clone_from(&target);
                    };

                    continue;
                }
            };

            break;
        }

        if let Some(homeserver) = homeserver_public_key {
            let url = if host.starts_with("localhost") {
                format!("http://{host}")
            } else {
                format!("https://{host}")
            };

            return Ok((homeserver, Url::parse(&url)?));
        }

        Err(Error::Generic("Could not resolve endpoint".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pkarr::{
        dns::{rdata::SVCB, Packet},
        mainline::{dht::DhtSettings, Testnet},
        Keypair, PkarrClient, Settings, SignedPacket,
    };
    use pubky_homeserver::Homeserver;

    #[tokio::test]
    async fn resolve_homeserver() {
        let testnet = Testnet::new(3);
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

        let mut svcb = SVCB::new(0, server_tld.as_str().try_into().unwrap());

        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "pubky".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            60 * 60,
            pkarr::dns::rdata::RData::SVCB(svcb),
        ));

        let signed_packet = SignedPacket::from_packet(&intermediate, &packet).unwrap();

        pkarr_client.publish(&signed_packet).await.unwrap();

        tokio::task::spawn_blocking(move || {
            let client = PubkyClient::test(&testnet);

            let pubky = Keypair::random();

            client
                .publish_pubky_homeserver(&pubky, &format!("pubky.{}", &intermediate.public_key()));

            let (public_key, url) = client
                .resolve_pubky_homeserver(&pubky.public_key())
                .unwrap();

            assert_eq!(public_key, server.public_key());
            assert_eq!(url.host_str(), Some("localhost"));
            assert_eq!(url.port(), Some(server.port()));
        })
        .await
        .expect("task failed")
    }
}
