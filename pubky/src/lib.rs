#![allow(unused)]

use std::{collections::HashMap, time::Duration};

use pkarr::{
    dns::{rdata::SVCB, Packet},
    mainline::{dht::DhtSettings, Testnet},
    Keypair, PkarrClient, PublicKey, Settings, SignedPacket,
};
use ureq::{Agent, Response};
use url::Url;

use pubky_common::auth::AuthnSignature;

mod error;

use error::{Error, Result};

const MAX_RECURSIVE_PUBKY_HOMESERVER_RESOLUTION: u8 = 3;

#[derive(Debug)]
pub struct Client {
    agent: Agent,
    pkarr: PkarrClient,
}

impl Client {
    pub fn new() -> Self {
        Self {
            agent: Agent::new(),
            pkarr: PkarrClient::new(Default::default()).unwrap(),
        }
    }

    pub fn test(testnet: &Testnet) -> Self {
        Self {
            agent: Agent::new(),
            pkarr: PkarrClient::new(Settings {
                dht: DhtSettings {
                    request_timeout: Some(Duration::from_millis(10)),
                    bootstrap: Some(testnet.bootstrap.to_owned()),
                    ..DhtSettings::default()
                },
                ..Settings::default()
            })
            .unwrap(),
        }
    }

    // === Public Methods ===

    // === Private Methods ===

    /// Publish the SVCB record for `_pubky.<public_key>`.
    fn publish_pubky_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        let mut packet = Packet::new_reply(0);

        if let Some(existing) = self.pkarr.resolve(&keypair.public_key())? {
            for answer in existing.packet().answers.iter().cloned() {
                if !answer.name.to_string().starts_with("_pubky") {
                    packet.answers.push(answer.into_owned())
                }
            }
        }

        let mut svcb = SVCB::new(0, host.try_into()?);

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
    fn resolve_pubky_homeserver(&self, pubky: &PublicKey) -> Result<(PublicKey, String)> {
        // TODO: cache the result of this function?

        let mut target = format!("_pubky.{}", pubky);
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
            return Ok((homeserver, host));
        }

        Err(Error::Generic("Could not resolve homeserver".to_string()))
    }

    fn fetch_direct(&self, method: HttpMethod, url: &str) -> Result<Response> {
        self.agent
            .request(method.into(), url)
            .call()
            .map_err(|_| Error::Generic("ureq error".to_string()))
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum HttpMethod {
    GET,
    PUT,
}

impl From<HttpMethod> for &str {
    fn from(value: HttpMethod) -> Self {
        match value {
            HttpMethod::GET => "GET",
            HttpMethod::PUT => "PUT",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pkarr::{mainline::Testnet, Keypair};
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
            let client = Client::test(&testnet);

            let pubky = Keypair::random();

            client
                .publish_pubky_homeserver(&pubky, &format!("pubky.{}", &intermediate.public_key()));

            let (public_key, host) = client
                .resolve_pubky_homeserver(&pubky.public_key())
                .unwrap();

            assert_eq!(public_key, server.public_key());
            assert!(host.starts_with("localhost"));
            assert!(host.ends_with(&server.port().to_string()));
        })
        .await
        .expect("task failed")
    }
}
