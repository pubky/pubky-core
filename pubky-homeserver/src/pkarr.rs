//! Pkarr related task

use pkarr::{dns::rdata::SVCB, SignedPacket};

use crate::config::Config;

pub async fn publish_server_packet(
    pkarr_client: &pkarr::Client,
    config: &Config,
    port: u16,
) -> anyhow::Result<()> {
    // TODO: Try to resolve first before publishing.

    let default = ".".to_string();
    let target = config.domain().unwrap_or(&default);
    let mut svcb = SVCB::new(0, target.as_str().try_into()?);

    svcb.priority = 1;
    svcb.set_port(port);

    let mut signed_packet_builder =
        SignedPacket::builder().https(".".try_into().unwrap(), svcb.clone(), 60 * 60);

    if config.domain().is_none() {
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

    let signed_packet = signed_packet_builder.build(config.keypair())?;

    pkarr_client.publish(&signed_packet).await?;

    Ok(())
}
