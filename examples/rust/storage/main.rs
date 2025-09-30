use anyhow::Result;
use clap::Parser;
use std::env;

use pubky::Pubky;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Pubky Resource
    resource: String,
    /// Use testnet mode
    #[clap(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(env::var("TRACING").unwrap_or("info".to_string()))
        .init();

    // For a basic GET request to any homeserver no session or key material is needed.
    // Create from a mainnet or testnet as needed.
    let storage = match args.testnet {
        true => Pubky::testnet()?.public_storage(),
        false => Pubky::new()?.public_storage(),
    };

    // Build the request
    let response = storage.get(args.resource).await?;

    println!("< Response:");
    println!("< {:?} {}", response.version(), response.status());
    for (name, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            println!("< {name}: {v}");
        }
    }

    let bytes = response.bytes().await?;

    match String::from_utf8(bytes.to_vec()) {
        Ok(string) => println!("<\n{}", string),
        Err(_) => println!("<\n{:?}", bytes),
    }

    Ok(())
}
