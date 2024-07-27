pub use pkarr::{
    dns::{rdata::SVCB, Packet},
    mainline::{dht::DhtSettings, Testnet},
    Keypair, PkarrClient, PublicKey, Settings, SignedPacket,
};

use crate::shared::pkarr::{format_url, parse_pubky_svcb, prepare_packet_for_signup};

use super::{Error, PubkyClient, Result, Url};

impl PubkyClient {
    /// Publish the SVCB record for `_pubky.<public_key>`.
    pub(crate) async fn publish_pubky_homeserver(
        &self,
        keypair: &Keypair,
        host: &str,
    ) -> Result<()> {
        let existing = self.pkarr.resolve(&keypair.public_key()).await?;

        let signed_packet = prepare_packet_for_signup(keypair, host, existing)?;

        self.pkarr.publish(&signed_packet).await?;

        Ok(())
    }

    /// Resolve the homeserver for a pubky.
    pub(crate) async fn resolve_pubky_homeserver(
        &self,
        pubky: &PublicKey,
    ) -> Result<(PublicKey, Url)> {
        let target = format!("_pubky.{}", pubky);

        self.resolve_endpoint(&target)
            .await
            .map_err(|_| Error::Generic("Could not resolve homeserver".to_string()))
    }

    /// Resolve a service's public_key and clearnet url from a Pubky domain
    pub(crate) async fn resolve_endpoint(&self, target: &str) -> Result<(PublicKey, Url)> {
        // TODO: cache the result of this function?

        let mut target = target.to_string();
        let mut homeserver_public_key = None;
        let mut host = target.clone();

        let mut step = 0;

        // PublicKey is very good at extracting the Pkarr TLD from a string.
        while let Ok(public_key) = PublicKey::try_from(target.clone()) {
            let response = self.pkarr.resolve(&public_key).await?;

            let done = parse_pubky_svcb(
                response,
                &public_key,
                &mut target,
                &mut homeserver_public_key,
                &mut host,
                &mut step,
            );

            if done {
                break;
            }
        }

        format_url(homeserver_public_key, host)
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

        {
            let client = PubkyClient::test(&testnet);

            let pubky = Keypair::random();

            client
                .publish_pubky_homeserver(&pubky, &format!("pubky.{}", &intermediate.public_key()))
                .await
                .unwrap();

            let (public_key, url) = client
                .resolve_pubky_homeserver(&pubky.public_key())
                .await
                .unwrap();

            assert_eq!(public_key, server.public_key());
            assert_eq!(url.host_str(), Some("localhost"));
            assert_eq!(url.port(), Some(server.port()));
        }
    }
}
