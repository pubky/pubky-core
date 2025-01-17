//! Pkarr related task

use anyhow::Result;
use pkarr::{dns::rdata::SVCB, SignedPacket};

use crate::Config;

pub struct PkarrServer {
    client: pkarr::Client,
    signed_packet: SignedPacket,
}

impl PkarrServer {
    pub fn new(config: &Config, https_port: u16, http_port: u16) -> Result<Self> {
        let mut dht_config = mainline::Config::default();

        if let Some(bootstrap) = config.bootstrap.clone() {
            dht_config.bootstrap = bootstrap;
        }
        if let Some(request_timeout) = config.dht_request_timeout {
            dht_config.request_timeout = request_timeout;
        }

        let client = pkarr::Client::builder().dht_config(dht_config).build()?;

        let signed_packet = create_signed_packet(config, https_port, http_port)?;

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

pub fn create_signed_packet(
    config: &Config,
    https_port: u16,
    http_port: u16,
) -> Result<SignedPacket> {
    // TODO: Try to resolve first before publishing.

    let mut signed_packet_builder = SignedPacket::builder();

    let mut svcb = SVCB::new(0, ".".try_into()?);

    // Set the public Ip or the loclahost
    signed_packet_builder = signed_packet_builder.address(
        ".".try_into().unwrap(),
        config
            .io
            .public_addr
            .map(|addr| addr.ip())
            .unwrap_or("127.0.0.1".parse().expect("localhost is valid ip")),
        60 * 60,
    );

    // Set the public port or the local https_port
    svcb.set_port(
        config
            .io
            .public_addr
            .map(|addr| addr.port())
            .unwrap_or(https_port),
    );

    signed_packet_builder = signed_packet_builder.https(".".try_into().unwrap(), svcb, 60 * 60);

    // Set low priority https record for legacy browsers support
    if config.testnet {
        let mut svcb = SVCB::new(10, ".".try_into()?);

        let http_port_be_bytes = http_port.to_be_bytes();
        svcb.set_param(
            pubky_common::constants::reserved_param_keys::HTTP_PORT,
            &http_port_be_bytes,
        )?;

        svcb.target = "localhost".try_into().expect("localhost is valid dns name");

        signed_packet_builder = signed_packet_builder.https(".".try_into().unwrap(), svcb, 60 * 60)
    } else if let Some(ref domain) = config.io.domain {
        let mut svcb = SVCB::new(10, ".".try_into()?);
        svcb.target = domain.as_str().try_into()?;

        signed_packet_builder = signed_packet_builder.https(".".try_into().unwrap(), svcb, 60 * 60);
    }

    Ok(signed_packet_builder.build(&config.keypair)?)
}