use anyhow::Result;
use base64::{alphabet::URL_SAFE, engine::general_purpose::NO_PAD, Engine};
use clap::Parser;
use pubky::PubkyClient;
use pubky_common::{capabilities::Capability, crypto::PublicKey};
use std::{collections::HashMap, path::PathBuf};
use url::Url;

/// local testnet HOMESERVER
const HOMESERVER: &str = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a recovery_file of the Pubky you want to sign in with
    recovery_file: PathBuf,

    /// Pubky Auth url
    url: Url,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    println!("\nSuccessfully opened recovery file");

    let url = cli.url;

    let query_params: HashMap<String, String> = url.query_pairs().into_owned().collect();

    let relay = query_params
        .get("relay")
        .map(|r| url::Url::parse(r).expect("Relay query param to be valid URL"))
        .expect("Missing relay query param");

    let client_secret = query_params
        .get("secret")
        .map(|s| {
            let engine = base64::engine::GeneralPurpose::new(&URL_SAFE, NO_PAD);
            let bytes = engine.decode(s).expect("invalid client_secret");
            let arr: [u8; 32] = bytes.try_into().expect("invalid client_secret");

            arr
        })
        .expect("Missing client secret");

    let required_capabilities = query_params
        .get("capabilities")
        .map(|caps_string| {
            caps_string
                .split(',')
                .filter_map(|cap| Capability::try_from(cap).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !required_capabilities.is_empty() {
        println!("\nRequired Capabilities:");
    }

    for cap in &required_capabilities {
        println!("    {} : {:?}", cap.scope, cap.abilities);
    }

    // === Consent form ===

    println!("\nEnter your recovery_file's passphrase to confirm:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky_common::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted recovery file...");
    println!("PublicKey: {}", keypair.public_key());

    let client = PubkyClient::testnet();

    // For the purposes of this demo, we need to make sure
    // the user has an account on the local homeserver.
    if client.signin(&keypair).await.is_err() {
        client
            .signup(&keypair, &PublicKey::try_from(HOMESERVER).unwrap())
            .await?;
    };

    println!("Sending AuthToken to the 3rd party app...");

    client
        .authorize(&keypair, required_capabilities, client_secret, &relay)
        .await?;

    Ok(())
}
