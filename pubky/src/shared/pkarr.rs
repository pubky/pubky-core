use pkarr::{
    dns::{rdata::SVCB, Packet},
    Keypair, SignedPacket,
};

use crate::{error::Result, Client};

impl Client {
    /// Publish the HTTPS record for `_pubky.<public_key>`.
    pub(crate) async fn publish_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        // TODO: Before making public, consider the effect on other records and other mirrors

        let existing = self.pkarr.resolve(&keypair.public_key()).await?;

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
            pkarr::dns::rdata::RData::HTTPS(svcb.into()),
        ));

        let signed_packet = SignedPacket::from_packet(keypair, &packet)?;

        self.pkarr.publish(&signed_packet).await?;

        Ok(())
    }
}
