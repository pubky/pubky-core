use wasm_bindgen::prelude::*;

use crate::actors::{auth_flow::AuthFlow, signer::Signer, storage::PublicStorage};
use crate::wrappers::keys::PublicKey;
use crate::{client::constructor::Client, js_error::JsResult, wrappers::keys::Keypair};

/// High-level entrypoint to the Pubky SDK.
#[wasm_bindgen]
pub struct Pubky(pub(crate) pubky::Pubky);

#[wasm_bindgen]
impl Pubky {
    /// Create a Pubky facade wired for **mainnet** defaults (public relays).
    ///
    /// @returns {Pubky}
    /// A new facade instance. Use this to create signers, start auth flows, etc.
    ///
    /// @example
    /// const pubky = new Pubky();
    /// const signer = pubky.signer(Keypair.random());
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsResult<Pubky> {
        let client = Client::new(None)?;
        Ok(Pubky(pubky::Pubky::with_client(client.0)))
    }

    /// Create a Pubky facade preconfigured for a **local testnet**.
    ///
    /// If `host` is provided, PKARR and HTTP endpoints are derived as `http://<host>:ports/...`.
    /// If omitted, `"localhost"` is assumed (handy for `cargo install pubky-testnet`).
    ///
    /// @param {string=} host Optional host (e.g. `"localhost"`, `"docker-host"`, `"127.0.0.1"`).
    /// @returns {Pubky}
    ///
    /// @example
    /// const pubky = Pubky.testnet();              // localhost default
    /// const pubky = Pubky.testnet("docker-host"); // custom hostname/IP
    #[wasm_bindgen(js_name = "testnet")]
    pub fn testnet(host: Option<String>) -> JsResult<Pubky> {
        let client = Client::testnet(host)?;
        Ok(Pubky(pubky::Pubky::with_client(client.0)))
    }

    /// Wrap an existing configured HTTP client into a Pubky facade.
    ///
    /// @param {Client} client A previously constructed client.
    /// @returns {Pubky}
    ///
    /// @example
    /// const client = Client.testnet();
    /// const pubky = Pubky.withClient(client);
    #[wasm_bindgen(js_name = "withClient")]
    pub fn with_client(client: &Client) -> Pubky {
        Pubky(pubky::Pubky::with_client(client.0.clone()))
    }

    /// Start a **pubkyauth** flow.
    ///
    /// Provide a **capabilities string** and (optionally) a relay base URL.
    /// The capabilities string is a comma-separated list of entries:
    /// `"<scope>:<actions>"`, where:
    /// - `scope` starts with `/` (e.g. `/pub/example.app/`).
    /// - `actions` is any combo of `r` and/or `w` (order normalized; `wr` -> `rw`).
    /// Pass `""` for no scopes (read-only public session).
    ///
    /// @param {string} capabilities Comma-separated caps, e.g. `"/pub/app/:rw,/pub/foo/file:r"`.
    /// @param {string=} relay Optional HTTP relay base (e.g. `"https://…/link/"`).
    /// @returns {AuthFlow}
    /// A running auth flow. Show `authorizationUrl` as QR/deeplink,
    /// then `awaitApproval()` to obtain a `Session`.
    ///
    /// @throws {PubkyError}
    /// - `{ name: "InvalidInput" }` for malformed capabilities or bad relay URL
    /// - `{ name: "RequestError" }` if the flow cannot be started (network/relay)
    ///
    /// @example
    /// const flow = pubky.startAuthFlow("/pub/my.app/:rw");
    /// renderQr(flow.authorizationUrl);
    /// const session = await flow.awaitApproval();
    #[wasm_bindgen(js_name = "startAuthFlow")]
    pub fn start_auth_flow(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Capabilities")] capabilities: String,
        relay: Option<String>,
    ) -> JsResult<AuthFlow> {
        let flow = AuthFlow::start_with_client(capabilities, relay, Some(self.0.client().clone()))?;
        Ok(flow)
    }

    /// Create a `Signer` from an existing `Keypair`.
    ///
    /// @param {Keypair} keypair The user’s keys.
    /// @returns {Signer}
    ///
    /// @example
    /// const signer = pubky.signer(Keypair.random());
    /// const session = await signer.signup(homeserverPk, null);
    #[wasm_bindgen(js_name = "signer")]
    pub fn signer(&self, keypair: &Keypair) -> Signer {
        Signer(self.0.signer(keypair.as_inner().clone()))
    }

    /// Public, unauthenticated storage API.
    ///
    /// Use for **read-only** public access via addressed paths:
    /// `"<user-z32>/pub/…"`.
    ///
    /// @returns {PublicStorage}
    ///
    /// @example
    /// const text = await pubky.publicStorage.getText(`${userPk.z32()}/pub/example.com/hello.txt`);
    #[wasm_bindgen(js_name = "publicStorage", getter)]
    pub fn public_storage(&self) -> PublicStorage {
        PublicStorage(self.0.public_storage())
    }

    /// Resolve the homeserver for a given public key (read-only).
    ///
    /// Uses an internal read-only Pkdns actor.
    ///
    /// @param {PublicKey} user
    /// @returns {Promise<PublicKey|undefined>} Homeserver public key (z32) or `undefined` if not found.
    #[wasm_bindgen(js_name = "getHomeserverOf")]
    pub async fn get_homeserver_of(&self, user_public_key: &PublicKey) -> Option<PublicKey> {
        self.0
            .get_homeserver_of(user_public_key.as_inner())
            .await
            .map(Into::into)
    }

    /// Access the underlying HTTP client (advanced).
    ///
    /// @returns {Client}
    /// Use this for low-level `fetch()` calls or testing with raw URLs.
    ///
    /// @example
    /// const r = await pubky.client.fetch(`pubky://${user}/pub/app/file.txt`, { credentials: "include" });
    #[wasm_bindgen(getter)]
    pub fn client(&self) -> Client {
        Client(self.0.client().clone())
    }
}
