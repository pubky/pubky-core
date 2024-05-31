//! Pkarr related functions

use pkarr::{PublicKey, SignedPacket};

use crate::{Error, Result};

pub fn homeserver(signed_packet: &SignedPacket) -> Result<PublicKey> {
    // TODO: cache this.
    for x in signed_packet.resource_records("_pk") {
        match &x.rdata {
            pkarr::dns::rdata::RData::TXT(txt) => {
                let attributes = txt.attributes();
                let home = attributes.get("home");

                if let Some(&Some(ref public_key)) = home {
                    return Ok(public_key.to_owned().try_into().unwrap());
                }
            }
            _ => {}
        }
    }

    Err(Error::Generic(
        "Didn't find any homeserver for this user".to_string(),
    ))
}
