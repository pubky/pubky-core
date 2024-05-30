//! Pkarr related tasks

use pk_common::Result;
use pkarr::{dns::Packet, Keypair, PkarrClientAsync, SignedPacket};

pub async fn publish_server_pkarr(
    pkarr_client: &PkarrClientAsync,
    keypair: &Keypair,
    domain: &str,
    port: u16,
) -> Result<()> {
    let mut packet = Packet::new_reply(0);

    // publishing port just to help unit tests!
    packet.answers.push(pkarr::dns::ResourceRecord::new(
        "".try_into().unwrap(),
        pkarr::dns::CLASS::IN,
        60 * 60,
        pkarr::dns::rdata::RData::CNAME(pkarr::dns::Name::new(domain).unwrap().into()),
    ));

    // publishing port just to help unit tests!
    if domain == "localhost" {
        packet.answers.push(pkarr::dns::ResourceRecord::new(
            "__PORT__".try_into().unwrap(),
            pkarr::dns::CLASS::IN,
            60 * 60,
            pkarr::dns::rdata::RData::A(pkarr::dns::rdata::A {
                address: port as u32,
            }),
        ));
    };

    let signed_packet = SignedPacket::from_packet(&keypair, &packet).unwrap();
    pkarr_client.publish(&signed_packet).await.unwrap();

    Ok(())
}
