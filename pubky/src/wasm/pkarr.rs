use wasm_bindgen::prelude::*;

pub use pkarr::{
    dns::{rdata::SVCB, Packet},
    Keypair, PublicKey, SignedPacket,
};

use crate::error::Result;

use super::PubkyClient;

// TODO: Share more code with the non-wasm client.

impl PubkyClient {
    /// Publish the SVCB record for `_pubky.<public_key>`.
    pub(crate) async fn publish_pubky_homeserver(
        &self,
        keypair: &Keypair,
        host: &str,
    ) -> Result<()> {
        let mut packet = Packet::new_reply(0);

        if let Some(existing) = self.pkarr.resolve(&keypair.public_key()).await? {
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

        let signed_packet = SignedPacket::from_packet(keypair, &packet)?;

        self.pkarr.publish(&signed_packet).await?;

        Ok(())
    }
}
