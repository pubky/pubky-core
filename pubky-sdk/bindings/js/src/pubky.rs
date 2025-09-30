use wasm_bindgen::prelude::*;

use crate::actors::{auth_flow::AuthFlow, pkdns::Pkdns, signer::Signer, storage::PublicStorage};
use crate::{client::constructor::Client, js_error::JsResult, wrappers::keys::Keypair};

#[wasm_bindgen]
pub struct Pubky(pub(crate) pubky::Pubky);

#[wasm_bindgen]
impl Pubky {
    /// Construct with defaults (mainnet pkarr relays).
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsResult<Pubky> {
        Ok(Pubky(pubky::Pubky::new()?))
    }

    /// Construct preconfigured for a local testnet.
    /// If `host` provided, pkarr and http relays -> `http://<host>:xxxxx/`
    /// If not provided `localhost` is used.
    #[wasm_bindgen(js_name = "testnet")]
    pub fn testnet(host: Option<String>) -> JsResult<Pubky> {
        let client = Client::testnet(host);
        Ok(Pubky(pubky::Pubky::with_client(client.0)))
    }

    /// Construct from an already-configured HTTP client.
    #[wasm_bindgen(js_name = "withClient")]
    pub fn with_client(client: &Client) -> Pubky {
        Pubky(pubky::Pubky::with_client(client.0.clone()))
    }

    /// Start an auth flow using this façade’s client.
    #[wasm_bindgen(js_name = "startAuthFlow")]
    pub fn start_auth_flow(&self, capabilities: &str, relay: Option<String>) -> JsResult<AuthFlow> {
        let flow = AuthFlow::start_with_client(capabilities, relay, Some(self.0.client().clone()))?;
        Ok(flow)
    }

    /// Create a signer bound to this façade’s client from an existing keypair.
    #[wasm_bindgen(js_name = "signer")]
    pub fn signer(&self, keypair: Keypair) -> Signer {
        Signer(self.0.signer(keypair.as_inner().clone()))
    }

    /// Public, unauthenticated storage bound to this façade’s client.
    #[wasm_bindgen(js_name = "publicStorage")]
    pub fn public_storage(&self) -> PublicStorage {
        PublicStorage(self.0.public_storage())
    }

    /// Read-only PKDNS actor bound to this façade’s client.
    #[wasm_bindgen]
    pub fn pkdns(&self) -> Pkdns {
        Pkdns(self.0.pkdns())
    }

    /// Expose the underlying HTTP client for advanced use.
    #[wasm_bindgen]
    pub fn client(&self) -> Client {
        Client(self.0.client().clone())
    }
}
