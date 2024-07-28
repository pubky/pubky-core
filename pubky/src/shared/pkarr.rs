use url::Url;

use pkarr::{
    dns::{rdata::SVCB, Packet},
    Keypair, PublicKey, SignedPacket,
};

use crate::error::{Error, Result};

const MAX_RECURSIVE_PUBKY_HOMESERVER_RESOLUTION: u8 = 3;

pub fn prepare_packet_for_signup(
    keypair: &Keypair,
    host: &str,
    existing: Option<SignedPacket>,
) -> Result<SignedPacket> {
    let mut packet = Packet::new_reply(0);

    if let Some(existing) = existing {
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

    Ok(SignedPacket::from_packet(keypair, &packet)?)
}

pub fn parse_pubky_svcb(
    signed_packet: Option<SignedPacket>,
    public_key: &PublicKey,
    target: &mut String,
    homeserver_public_key: &mut Option<PublicKey>,
    host: &mut String,
    step: &mut u8,
) -> bool {
    *step += 1;

    let mut prior = None;

    if let Some(signed_packet) = signed_packet {
        for answer in signed_packet.resource_records(target) {
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
            *homeserver_public_key = Some(public_key.clone());
            *target = svcb.target.to_string();

            if let Some(port) = svcb.get_param(pkarr::dns::rdata::SVCB::PORT) {
                if port.len() < 2 {
                    // TODO: debug! Error encoding port!
                }
                let port = u16::from_be_bytes([port[0], port[1]]);

                *host = format!("{target}:{port}");
            } else {
                host.clone_from(target);
            };

            return *step >= MAX_RECURSIVE_PUBKY_HOMESERVER_RESOLUTION;
        }
    }

    true
}

pub fn format_url(
    original_target: &str,
    homeserver_public_key: Option<PublicKey>,
    host: String,
) -> Result<(PublicKey, Url)> {
    if let Some(homeserver) = homeserver_public_key {
        let url = if host.starts_with("localhost") {
            format!("http://{host}")
        } else {
            format!("https://{host}")
        };

        return Ok((homeserver, Url::parse(&url)?));
    }

    Err(Error::ResolveEndpoint(original_target.into()))
}
