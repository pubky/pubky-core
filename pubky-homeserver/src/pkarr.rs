//! Pkarr related task

use pkarr::{
    dns::{rdata::SVCB, Packet},
    Keypair, PkarrClientAsync, SignedPacket,
};

pub(crate) async fn publish_server_packet(
    pkarr_client: &PkarrClientAsync,
    keypair: &Keypair,
    domain: &str,
    port: u16,
) -> anyhow::Result<()> {
    // TODO: Try to resolve first before publishing.

    let mut packet = Packet::new_reply(0);

    let mut svcb = SVCB::new(0, domain.try_into()?);

    // Publishing port only for localhost domain,
    // assuming any other domain will point to a reverse proxy
    // at the conventional ports.
    if domain == "localhost" {
        svcb.priority = 1;
        svcb.set_port(port);

        // TODO: Add more parameteres like the signer key!
        // svcb.set_param(key, value)
    };

    // TODO: announce A/AAAA records as well for Noise connections?
    // Or maybe Iroh's magic socket

    packet.answers.push(pkarr::dns::ResourceRecord::new(
        "@".try_into().unwrap(),
        pkarr::dns::CLASS::IN,
        60 * 60,
        pkarr::dns::rdata::RData::SVCB(svcb),
    ));

    let signed_packet = SignedPacket::from_packet(keypair, &packet)?;

    pkarr_client.publish(&signed_packet).await?;

    Ok(())
}
