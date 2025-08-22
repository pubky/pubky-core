use std::{env, sync::Arc};

use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use url::Url;

use pubky::{Client, KeylessAgent};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// HTTP method to use
    method: Method,
    /// Pubky or HTTPS url
    url: Url,
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
    let agent = if args.testnet {
        KeylessAgent::with_client(Arc::new(Client::testnet()?))
    } else {
        KeylessAgent::new()?
    };

    // Build the request
    let response = agent.homeserver().get(args.url.as_str()).await?;

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
