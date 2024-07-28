use reqwest::StatusCode;
use url::Url;
use wasm_bindgen::prelude::*;

pub use pkarr::{
    dns::{rdata::SVCB, Packet},
    Keypair, PublicKey, SignedPacket,
};

use crate::error::{Error, Result};
use crate::shared::pkarr::{format_url, parse_pubky_svcb, prepare_packet_for_signup};
use crate::PubkyClient;

const TEST_RELAY: &str = "http://localhost:15411/pkarr";

#[macro_export]
macro_rules! log {
    ($($arg:expr),*) => {
        web_sys::console::debug_1(&format!($($arg),*).into());
    };
}

impl PubkyClient {
    /// Publish the SVCB record for `_pubky.<public_key>`.
    pub(crate) async fn publish_pubky_homeserver(
        &self,
        keypair: &Keypair,
        host: &str,
    ) -> Result<()> {
        // let existing = self.pkarr.resolve(&keypair.public_key()).await?;
        let existing = self.pkarr_resolve(&keypair.public_key()).await?;

        let signed_packet = prepare_packet_for_signup(keypair, host, existing)?;

        // self.pkarr.publish(&signed_packet).await?;
        self.pkarr_publish(&signed_packet).await?;

        Ok(())
    }

    /// Resolve the homeserver for a pubky.
    pub(crate) async fn resolve_pubky_homeserver(
        &self,
        pubky: &PublicKey,
    ) -> Result<(PublicKey, Url)> {
        let target = format!("_pubky.{}", pubky);

        self.resolve_endpoint(&target)
            .await
            .map_err(|_| Error::Generic("Could not resolve homeserver".to_string()))
    }

    /// Resolve a service's public_key and clearnet url from a Pubky domain
    pub(crate) async fn resolve_endpoint(&self, target: &str) -> Result<(PublicKey, Url)> {
        let original_target = target;
        // TODO: cache the result of this function?

        let mut target = target.to_string();
        let mut homeserver_public_key = None;
        let mut host = target.clone();

        let mut step = 0;

        // PublicKey is very good at extracting the Pkarr TLD from a string.
        while let Ok(public_key) = PublicKey::try_from(target.clone()) {
            let response = self
                .pkarr_resolve(&public_key)
                .await
                .map_err(|e| Error::ResolveEndpoint(original_target.into()))?;

            let done = parse_pubky_svcb(
                response,
                &public_key,
                &mut target,
                &mut homeserver_public_key,
                &mut host,
                &mut step,
            );

            if done {
                break;
            }
        }

        format_url(original_target, homeserver_public_key, host)
    }

    //TODO: Allow multiple relays in parallel
    //TODO: migrate to pkarr::PkarrRelayClient
    async fn pkarr_resolve(&self, public_key: &PublicKey) -> Result<Option<SignedPacket>> {
        let res = self
            .http
            .get(format!("{TEST_RELAY}/{}", public_key))
            .send()
            .await?;

        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        };

        // TODO: guard against too large responses.
        let bytes = res.bytes().await?;

        let existing = SignedPacket::from_relay_payload(public_key, &bytes)?;

        Ok(Some(existing))
    }

    async fn pkarr_publish(&self, signed_packet: &SignedPacket) -> Result<()> {
        self.http
            .put(format!("{TEST_RELAY}/{}", signed_packet.public_key()))
            .body(signed_packet.to_relay_payload())
            .send()
            .await?;

        Ok(())
    }
}
