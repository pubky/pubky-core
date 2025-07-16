use anyhow::Result;
use clap::Parser;
use pubky::Client;
use std::path::PathBuf;
use url::Url;

use pubky_common::{capabilities::Capability, crypto::PublicKey};

/// local testnet HOMESERVER
const HOMESERVER: &str = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a recovery_file of the Pubky you want to sign in with
    recovery_file: PathBuf,

    /// Pubky Auth url
    url: Url,

    /// Use testnet mode
    #[clap(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let homeserver = PublicKey::try_from(HOMESERVER).unwrap();

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    println!("\nSuccessfully opened recovery file");

    let url = cli.url;

    let caps = url
        .query_pairs()
        .filter_map(|(key, value)| {
            if key == "caps" {
                return Some(
                    value
                        .split(',')
                        .filter_map(|cap| Capability::try_from(cap).ok())
                        .collect::<Vec<_>>(),
                );
            };
            None
        })
        .next()
        .unwrap_or_default();

    if !caps.is_empty() {
        println!("\nRequired Capabilities:");
    }

    for cap in &caps {
        println!("    {} : {:?}", cap.scope, cap.actions);
    }

    // === Consent form ===

    println!("\nEnter your recovery_file's passphrase to confirm:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky_common::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted recovery file...");
    println!("PublicKey: {}", keypair.public_key());

    let client = if cli.testnet {
        let client = Client::builder().testnet().build()?;

        // For the purposes of this demo, we need to make sure
        // the user has an account on the local homeserver.

        if client.signin(&keypair).await.is_err() {
            client.signup(&keypair, &homeserver).send().await?;
        };

        client
    } else {
        Client::builder().build()?
    };

    println!("Sending AuthToken to the 3rd party app...");

    client.send_auth_token(&keypair, &url).await?;

    Ok(())
}
