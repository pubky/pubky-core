use anyhow::Result;
use clap::Parser;
use pubky::PubkyClient;
use pubky_common::{auth::AuthToken, capabilities::Capability, crypto::PublicKey};
use std::path::PathBuf;
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

    let url = cli.url;

    let mut required_capabilities = vec![];
    let mut relay = "".to_string();
    let client_secret: [u8; 32];

    for (name, value) in url.query_pairs() {
        if name == "relay" {
            relay = value.to_string();
        }
        if name == "secret" {
            // client_secret = value.to_string();
        }
        if name == "capabilities" {
            println!("\nRequired Capabilities:");

            for cap_str in value.split(',') {
                if let Ok(cap) = Capability::try_from(cap_str) {
                    println!("    {} : {:?}", cap.resource, cap.abilities);
                    required_capabilities.push(cap)
                };
            }
        }
    }

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    // println!("\nSuccessfully opened recovery file");

    // === Consent form ===

    println!("\nEnter your recovery_file's passphrase to confirm:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky_common::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted recovery file...");

    let client = PubkyClient::testnet();

    // For the purposes of this demo, we need to make sure
    // the user has an account on the local homeserver.
    if client.signin(&keypair).await.is_err() {
        client
            .signup(&keypair, &PublicKey::try_from(HOMESERVER).unwrap())
            .await?;
    };

    client
        .authorize(&keypair, required_capabilities, [0; 32], &relay)
        .await?;

    println!("Sending AuthToken to the client...");

    Ok(())
}
