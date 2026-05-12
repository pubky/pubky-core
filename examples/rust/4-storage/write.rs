use anyhow::Result;
use clap::Parser;
use pubky::Pubky;
use std::path::PathBuf;

#[derive(Parser, Debug)]
pub struct Args {
    /// Path to the recovery file
    recovery_file: PathBuf,

    /// Resource path to write to (e.g. /pub/example/hello.txt)
    #[arg(default_value = "/pub/example/hello.txt")]
    path: String,

    /// Content to write
    #[arg(short, long, default_value = "Hello from pubky!")]
    content: String,

    /// Use the local testnet defaults instead of mainnet relays
    #[arg(long)]
    testnet: bool,
}

pub async fn run(args: Args) -> Result<()> {
    // 1) Load and decrypt the recovery file
    let recovery_bytes = std::fs::read(&args.recovery_file)?;
    println!("Enter your recovery file passphrase:");
    let passphrase = rpassword::read_password()?;
    let keypair = pubky::recovery_file::decrypt_recovery_file(&recovery_bytes, &passphrase)?;
    println!("Decrypted recovery file for {}", keypair.public_key());

    // 2) Sign in
    let pubky = if args.testnet {
        Pubky::testnet()?
    } else {
        Pubky::new()?
    };

    let signer = pubky.signer(keypair);
    let session = signer.signin().await?;
    println!("Signed in successfully!");

    let storage = session.storage();

    // 3) PUT - write content
    println!("\nPUT {} ...", args.path);
    storage
        .put(&args.path, args.content.as_bytes().to_vec())
        .await?;
    println!("  Written successfully.");

    // 4) GET - read it back
    println!("\nGET {} ...", args.path);
    let response = storage.get(&args.path).await?;
    let body = response.bytes().await?;
    println!("  Content: {}", String::from_utf8_lossy(&body));

    // 5) DELETE - clean up
    println!("\nDELETE {} ...", args.path);
    storage.delete(&args.path).await?;
    println!("  Deleted successfully.");

    Ok(())
}
