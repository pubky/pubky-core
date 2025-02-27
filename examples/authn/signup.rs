use anyhow::Result;
use clap::Parser;
use pubky::{Client, PublicKey};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Homeserver Pkarr Domain (for example `5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo`)
    homeserver: String,

    /// Path to a recovery_file of the Pubky you want to sign in with
    recovery_file: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    println!("\nSuccessfully opened recovery file");

    let homeserver = cli.homeserver;

    let client = Client::builder().build()?;

    println!("Enter your recovery_file's passphrase to signup:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted the recovery file, signing up to the homeserver:");

    client
        .signup(&keypair, &PublicKey::try_from(homeserver).unwrap(), None)
        .await?;

    println!("Successfully signed up. Checking session:");

    let session = client.session(&keypair.public_key()).await?;

    println!("Successfully resolved current session at the homeserver.");

    println!("{:?}", session);

    Ok(())
}
