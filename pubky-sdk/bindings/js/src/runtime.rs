use crate::js_error::PubkyJsError;
use wasm_bindgen::prelude::*;

/// Configure the SDK's global HTTP client to talk to a local Pubky testnet.
/// - Sets PKARR relays to `http://<host>:15411/`
/// - On WASM, also sets the testnet hostname so `pubky://` is rewritten to `http://<host>:<port>`
///
/// Call this **before** constructing `Signer` / using any API that relies on the global client.
#[wasm_bindgen(js_name = "useTestnet")]
pub fn use_testnet(host: Option<String>) -> Result<(), PubkyJsError> {
    let host = host.unwrap_or_else(|| "localhost".to_string());
    let relay = format!("http://{}:{}/", host, 15411);

    let mut builder = pubky::PubkyHttpClient::builder();
    builder.pkarr(|p| p.relays(&[relay.as_str()]).expect("valid testnet relay"));

    // WASM-only hint so the client knows to use http + pkarr HTTP_PORT for localhost
    #[cfg(target_arch = "wasm32")]
    {
        builder.testnet_host(host);
    }

    let client = builder.build()?;
    pubky::set_global_client(client);
    Ok(())
}
