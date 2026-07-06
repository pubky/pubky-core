use anyhow::Result;
use clap::Parser;
use pubky::{ClientId, Pubky};
use std::path::PathBuf;

#[path = "../recovery.rs"]
mod recovery;

#[derive(Parser, Debug)]
pub struct Args {
    /// Resource path, or a custom recovery file when followed by a resource path
    #[arg(value_name = "PATH_OR_RECOVERY_FILE")]
    first: Option<String>,

    /// Resource path when using a custom recovery file
    #[arg(value_name = "PATH")]
    second: Option<String>,

    /// Content to write
    #[arg(short, long, default_value = "Hello from pubky!")]
    content: String,

    /// Use the local testnet defaults instead of mainnet relays
    #[arg(long)]
    testnet: bool,
}

pub async fn run(args: Args) -> Result<()> {
    let (recovery_file, path) = write_args(&args);

    // 1) Load and decrypt the recovery file
    let keypair =
        recovery::decrypt_recovery_file(&recovery_file, "Enter your recovery file passphrase:")?;
    println!("Decrypted recovery file for {}", keypair.public_key());

    // 2) Sign in
    let pubky = if args.testnet {
        Pubky::testnet()?
    } else {
        Pubky::new()?
    };

    let signer = pubky.signer(keypair);
    let session = signer.signin(ClientId::new("storage.example")?).await?;
    println!("Signed in successfully!");

    let storage = session.storage();

    // 3) PUT - write content
    println!("\nPUT {} ...", path);
    storage.put(&path, args.content.as_bytes().to_vec()).await?;
    println!("  Written successfully.");

    // 4) GET - read it back
    println!("\nGET {} ...", path);
    let response = storage.get(&path).await?;
    let body = response.bytes().await?;
    println!("  Content: {}", String::from_utf8_lossy(&body));

    // 5) DELETE - clean up
    println!("\nDELETE {} ...", path);
    storage.delete(&path).await?;
    println!("  Deleted successfully.");

    Ok(())
}

fn write_args(args: &Args) -> (PathBuf, String) {
    match (&args.first, &args.second) {
        (Some(first), Some(second)) => (PathBuf::from(first), second.clone()),
        (Some(first), None) if first.starts_with('/') => {
            (recovery::sample_recovery_file(), first.clone())
        }
        (Some(first), None) => (PathBuf::from(first), default_path()),
        (None, _) => (recovery::sample_recovery_file(), default_path()),
    }
}

fn default_path() -> String {
    "/pub/example/hello.txt".to_string()
}
