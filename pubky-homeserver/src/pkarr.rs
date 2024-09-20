//! Pkarr related task

use std::net::Ipv4Addr;

use pkarr::{
    dns::{
        rdata::{RData, A, SVCB},
        Packet,
    },
    Keypair, PkarrClientAsync, SignedPacket,
};

pub async fn publish_server_packet(
    pkarr_client: PkarrClientAsync,
    keypair: &Keypair,
    domain: Option<&String>,
    port: u16,
) -> anyhow::Result<()> {
    // TODO: Try to resolve first before publishing.

    let mut packet = Packet::new_reply(0);

    let default = ".".to_string();
    let target = domain.unwrap_or(&default);
    let mut svcb = SVCB::new(0, target.as_str().try_into()?);

    svcb.priority = 1;
    svcb.set_port(port);

    packet.answers.push(pkarr::dns::ResourceRecord::new(
        "@".try_into().unwrap(),
        pkarr::dns::CLASS::IN,
        60 * 60,
        RData::HTTPS(svcb.clone().into()),
    ));

    if domain.is_none() {
        // TODO: remove after remvoing Pubky shared/public
        // and add local host IP address instead.
        svcb.target = "localhost".try_into().unwrap();

        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "@".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            60 * 60,
            RData::HTTPS(svcb.clone().into()),
        ));

        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "@".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            60 * 60,
            RData::A(A::from(Ipv4Addr::from([127, 0, 0, 1]))),
        ));
    }

    // TODO: announce A/AAAA records as well for TLS connections?

    let signed_packet = SignedPacket::from_packet(keypair, &packet)?;

    pkarr_client.publish(&signed_packet).await?;

    Ok(())
}
