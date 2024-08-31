use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use url::Url;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Pubky Auth url
    url: Url,

    /// Path to a recovery_file of the Pubky you want to sign in with
    // #[arg(short, long, value_name = "FILE")]
    recovery_file: PathBuf,
    // /// Mutable data public key.
    // public_key: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let url = cli.url;
    dbg!(url);

    let recovery_file = std::fs::read(&cli.recovery_file)?;
    println!("Successfully opened recovery file");

    // // println!("Enter Pubky Auth URL to start the consent form:");
    // // let pubky_auth_url = rl.readline("> ")?;
    // // dbg!(pubky_auth_url);

    println!("Enter your recovery_file's passphrase to confirm:");
    let passphrase = rpassword::read_password()?;

    let keypair = pubky_common::recovery_file::decrypt_recovery_file(&recovery_file, &passphrase)?;

    println!("Successfully decrypted recovery file...");

    Ok(())
}
