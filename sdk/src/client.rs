use std::collections::HashMap;

use pkarr::{
    dns::rdata::{A, TXT},
    mainline::{dht::DhtSettings, Testnet},
    Keypair, PkarrClient, PublicKey, Settings, SignedPacket,
};
use ureq::{Agent, Response};

use pk_common::{homeserver::auth::AuthnSignature, url::PkUrl, Error, Result};

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
                    bootstrap: Some(testnet.bootstrap.to_owned()),
                    ..DhtSettings::default()
                },
                ..Settings::default()
            })
            .unwrap(),
        }
    }

    pub fn register(&self, keypair: &Keypair, homeserver: &str) -> Result<Response> {
        let homeserver_public_key: PublicKey = homeserver
            .try_into()
            .map_err(|_| Error::Generic("homesever url is wrong".to_string()))?;

        let url = format!(
            "{}/register",
            self.homeserver_base_url(&homeserver_public_key)?
        );

        // TOOD: check previous packet first to avoid overriding it.
        // let previous_packet = self.pkarr.resolve(&keypair.public_key());

        let mut packet = pkarr::dns::Packet::new_reply(0);

        // TODO: is this the best way to generate this packet?
        let mut x = HashMap::with_capacity(1);
        x.insert("home".to_string(), Some(homeserver.to_string()));

        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "_pk".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            3600,
            pkarr::dns::rdata::RData::TXT(pkarr::dns::rdata::TXT::try_from(x).unwrap()),
        ));

        let signed_packet = SignedPacket::from_packet(keypair, &packet).unwrap();

        Ok(self
            .agent
            .request(HttpMethod::PUT.into(), &url)
            .send_bytes(signed_packet.as_bytes())
            .map_err(|e| Error::Generic("ureq error".to_string()))?)
    }

    pub fn authn(&self, keypair: &Keypair) -> Result<Response> {
        let public_key = keypair.public_key();
        let homeserver = self.user_homeserver(&public_key).unwrap();

        let signature = AuthnSignature::bearer(keypair, &homeserver);

        let base_url = self.homeserver_base_url(&homeserver).unwrap();
        let url = format!("{}/authn", base_url);

        Ok(self
            .agent
            .request(HttpMethod::PUT.into(), &url)
            .send_bytes(signature.as_bytes())
            .map_err(|e| Error::Generic("ureq error".to_string()))?)
    }

    fn fetch_direct(&self, method: HttpMethod, url: &str) -> Result<Response> {
        Ok(self
            .agent
            .request(method.into(), url)
            .call()
            .map_err(|e| Error::Generic("ureq error".to_string()))?)
    }

    fn user_homeserver(&self, public_key: &PublicKey) -> Result<PublicKey> {
        let signed_packet = self.pkarr.resolve(public_key).unwrap().unwrap();

        pk_common::pkarr::homeserver(&signed_packet)
    }

    /// Takes a [PublicKey] and returns the actual domain it is listening on.
    fn homeserver_base_url(&self, public_key: &PublicKey) -> Result<String> {
        if let Ok(Some(signed_packet)) = self.pkarr.resolve(&public_key) {
            // TODO: cache results
            let cname = match &signed_packet.resource_records(".").next().unwrap().rdata {
                pkarr::dns::rdata::RData::CNAME(name) => Some(name.to_string()),
                _ => None,
            }
            .unwrap();

            return if cname == "localhost" {
                let port = match signed_packet
                    .resource_records("__PORT__")
                    .next()
                    .unwrap()
                    .rdata
                {
                    pkarr::dns::rdata::RData::A(A { address }) => Some(address),
                    _ => None,
                }
                .unwrap();

                Ok(format!("http://{}:{}", cname, port))
            } else {
                Ok(format!("https://{}", cname))
            };
        };

        Err(Error::Generic("Couldn't find homeserver".to_string()))
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

    use pk_homeserver::Homeserver;
    use pkarr::{mainline::Testnet, Keypair};

    #[tokio::test]
    async fn direct_register() {
        let testnet = Testnet::new(3);
        let server = Homeserver::start_test(&testnet).await.unwrap();

        let server_pk = server.public_key().to_string();

        tokio::task::spawn_blocking(move || {
            let keypair = Keypair::random();

            let xx = Client::test(&testnet)
                .register(&keypair, &server_pk)
                .unwrap();

            dbg!(&keypair.public_key());

            let yy = Client::test(&testnet).authn(&keypair).unwrap();

            dbg!(yy);
        })
        .await
        .expect("task failed")
    }

    // #[tokio::test]
    // async fn basic() {
    //     let testnet = Testnet::new(3);
    //     let server = Homeserver::start_test(&testnet).await.unwrap();
    //
    //     let url = server.public_key().to_string();
    //
    //     tokio::task::spawn_blocking(move || {
    //         let keypair = Keypair::random();
    //
    //         let xx = Client::test(&testnet)
    //             .register(&keypair, &server.public_key())
    //             .unwrap();
    //
    //         dbg!(xx);
    //     })
    //     .await
    //     .expect("task failed")
    // }
}
