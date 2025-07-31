use std::env;

use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use url::Url;

use pubky::Client;

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

    let client = if args.testnet {
        Client::testnet()?
    } else {
        Client::default()
    };

    // Build the request
    println!("> {} {}", args.method, args.url);
    let response = client.request(args.method, args.url.as_str(), None).await?;

    println!("< Response:");
    println!("< {}", response.status);

    // Iterate over the .headers field.
    for (name, value) in &response.headers {
        if let Ok(v) = value.to_str() {
            println!("< {}: {}", name, v);
        }
    }
    println!("<");

    let bytes = response.body;

    match String::from_utf8(bytes) {
        Ok(string) => println!("{}", string),
        Err(e) => println!("{:?}", e.into_bytes()),
    }

    Ok(())
}
