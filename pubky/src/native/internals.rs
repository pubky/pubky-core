use pkarr::SignedPacket;
use pubky_common::crypto::PublicKey;
use reqwest::{RequestBuilder, Response};
use url::Url;

use crate::error::Result;
use crate::PubkyClient;

impl PubkyClient {
    // === Pkarr ===

    pub(crate) async fn pkarr_resolve(
        &self,
        public_key: &PublicKey,
    ) -> Result<Option<SignedPacket>> {
        Ok(self.pkarr.resolve(public_key).await?)
    }

    pub(crate) async fn pkarr_publish(&self, signed_packet: &SignedPacket) -> Result<()> {
        Ok(self.pkarr.publish(signed_packet).await?)
    }

    // === HTTP ===

    pub(crate) fn request(&self, method: reqwest::Method, url: Url) -> RequestBuilder {
        self.http.request(method, url)
    }

    pub(crate) fn store_session(&self, _: &Response) {}
    pub(crate) fn remove_session(&self, _: &PublicKey) {}
}
