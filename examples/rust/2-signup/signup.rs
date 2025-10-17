use anyhow::Result;
use clap::Parser;
use pubky::{Pubky, PublicKey};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Homeserver Pkarr Domain (for example `5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo`)
    homeserver: String,

    /// Path to a recovery_file of the Pubky you want to sign in with
    recovery_file: PathBuf,

    /// Signup code (optional)
    signup_code: Option<String>,

    /// Use the local testnet defaults instead of mainnet relays.
    #[arg(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    println!("\nSuccessfully opened recovery file");

    let homeserver = &PublicKey::try_from(cli.homeserver).unwrap();

    println!("Enter your recovery_file's passphrase to signup:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted the recovery file, signing up to the homeserver:");

    let pubky = if cli.testnet {
        Pubky::testnet()?
    } else {
        Pubky::new()?
    };

    let signer = pubky.signer(keypair);
    let session = signer
        .signup(homeserver, cli.signup_code.as_deref())
        .await?;

    println!("Successfully signed up. Checking session:");

    let session_info = session.info();

    println!("Successfully resolved current session at the homeserver.");
    println!("{:?}", session_info);

    Ok(())
}
