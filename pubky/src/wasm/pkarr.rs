use wasm_bindgen::prelude::*;

pub use pkarr::{
    dns::{rdata::SVCB, Packet},
    PkarrRelayClient, PublicKey, SignedPacket,
};

use crate::error::Result;
use crate::shared::pkarr::{format_url, parse_pubky_svcb, prepare_packet_for_signup};

use super::{keys::Keypair, PubkyClient};

impl PubkyClient {
    /// Publish the SVCB record for `_pubky.<public_key>`.
    pub(crate) async fn publish_pubky_homeserver(
        &self,
        keypair: &Keypair,
        host: &str,
    ) -> Result<()> {
        let existing = self.pkarr.resolve(&keypair.public_key().as_inner()).await?;

        let signed_packet = prepare_packet_for_signup(keypair.as_inner(), host, existing)?;

        self.pkarr.publish(&signed_packet).await?;

        Ok(())
    }
}
