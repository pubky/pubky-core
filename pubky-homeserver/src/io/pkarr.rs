//! Pkarr related task

use anyhow::Result;
use pkarr::{dns::rdata::SVCB, SignedPacket};

use crate::Config;

pub struct PkarrServer {
    client: pkarr::Client,
    signed_packet: SignedPacket,
}

impl PkarrServer {
    pub fn new(config: Config, port: u16) -> Result<Self> {
        let mut dht_config = pkarr::mainline::Config::default();

        if let Some(bootstrap) = config.bootstrap.clone() {
            dht_config.bootstrap = bootstrap;
        }
        if let Some(request_timeout) = config.dht_request_timeout {
            dht_config.request_timeout = request_timeout;
        }

        let client = pkarr::Client::builder().dht_config(dht_config).build()?;

        let signed_packet = create_signed_packet(config, port)?;

        Ok(Self {
            client,
            signed_packet,
        })
    }

    pub async fn publish_server_packet(&self) -> anyhow::Result<()> {
        // TODO: warn if packet is not most recent, which means the
        // user is publishing a Packet from somewhere else.

        self.client.publish(&self.signed_packet).await?;

        Ok(())
    }
}

pub fn create_signed_packet(config: Config, port: u16) -> Result<SignedPacket> {
    // TODO: Try to resolve first before publishing.

    let default = ".".to_string();
    let target = config.domain.clone().unwrap_or(default);
    let mut svcb = SVCB::new(0, target.as_str().try_into()?);

    svcb.priority = 1;
    svcb.set_port(port);

    let mut signed_packet_builder =
        SignedPacket::builder().https(".".try_into().unwrap(), svcb.clone(), 60 * 60);

    if config.domain.is_none() {
        // TODO: remove after remvoing Pubky shared/public
        // and add local host IP address instead.
        svcb.target = "localhost".try_into().unwrap();

        signed_packet_builder = signed_packet_builder
            .https(".".try_into().unwrap(), svcb, 60 * 60)
            .address(
                ".".try_into().unwrap(),
                "127.0.0.1".parse().unwrap(),
                60 * 60,
            );
    }

    // TODO: announce A/AAAA records as well for TLS connections?

    Ok(signed_packet_builder.build(&config.keypair)?)
}
