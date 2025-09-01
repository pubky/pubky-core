use anyhow::Result;
use clap::Parser;
use pubky::{Capabilities, PubkyClient, PubkySigner, PublicKey};
use std::{path::PathBuf, sync::Arc};
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

    /// Use testnet mode
    #[clap(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    println!("\nSuccessfully opened recovery file");

    let homeserver = &PublicKey::try_from(HOMESERVER).unwrap();
    let url = cli.url;

    let caps = Capabilities::from(&url);

    if !caps.is_empty() {
        println!("\nRequested capabilities:\n  {}", caps);
    }

    // === Consent form ===

    println!("\nEnter your recovery_file's passphrase to confirm:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky_common::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted recovery file...");
    println!("PublicKey: {}", keypair.public_key());

    let signer = if cli.testnet {
        let client = PubkyClient::testnet()?;
        let signer = PubkySigner::with_client(Arc::new(client), keypair);

        // For the purposes of this demo, we need to make sure
        // the user has an account on the local homeserver.
        signer.signup(homeserver, None).await?;

        signer
    } else {
        PubkySigner::new(keypair)?
    };

    println!("Sending AuthToken to the 3rd party app...");

    signer.send_auth_token(&url).await?;

    Ok(())
}
