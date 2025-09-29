use wasm_bindgen::prelude::*;

use crate::js_result::JsResult;
use crate::wrappers::keys::{Keypair, PublicKey};

#[wasm_bindgen]
pub struct Pkdns(pub(crate) pubky::Pkdns);

#[wasm_bindgen]
impl Pkdns {
    /// Read-only PKDNS actor (no keypair; resolve only).
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsResult<Pkdns> {
        Ok(Pkdns(pubky::Pkdns::new()?))
    }

    /// PKDNS actor with publishing enabled (requires a keypair).
    #[wasm_bindgen(js_name = "fromKeypair")]
    pub fn from_keypair(keypair: &Keypair) -> JsResult<Pkdns> {
        Ok(Pkdns(pubky::Pkdns::new_with_keypair(
            keypair.as_inner().clone(),
        )?))
    }

    // -------------------- Reads --------------------

    /// Resolve current homeserver for any pubkey via PKDNS.
    /// Returns the target host string, or `undefined` if not found.
    #[wasm_bindgen(js_name = "getHomeserverOf")]
    pub async fn get_homeserver_of(&self, pubky: &PublicKey) -> JsResult<Option<String>> {
        Ok(self.0.get_homeserver_of(pubky.as_inner()).await)
    }

    /// Convenience: resolve homeserver for **this** user (requires keypair).
    #[wasm_bindgen(js_name = "getHomeserver")]
    pub async fn get_homeserver(&self) -> JsResult<Option<String>> {
        Ok(self.0.get_homeserver().await?)
    }

    // -------------------- Publishing --------------------

    /// Force publish `_pubky` to the DHT. Optional host override.
    #[wasm_bindgen(js_name = "publishHomeserverForce")]
    pub async fn publish_homeserver_force(&self, host_override: Option<PublicKey>) -> JsResult<()> {
        let host_ref = host_override.as_ref().map(|h| h.as_inner());
        self.0.publish_homeserver_force(host_ref).await?;
        Ok(())
    }

    /// Publish `_pubky` only if missing or stale. Optional host override.
    #[wasm_bindgen(js_name = "publishHomeserverIfStale")]
    pub async fn publish_homeserver_if_stale(
        &self,
        host_override: Option<PublicKey>,
    ) -> JsResult<()> {
        let host_ref = host_override.as_ref().map(|h| h.as_inner());
        self.0.publish_homeserver_if_stale(host_ref).await?;
        Ok(())
    }
}

impl From<pubky::Pkdns> for Pkdns {
    fn from(inner: pubky::Pkdns) -> Self {
        Pkdns(inner)
    }
}
