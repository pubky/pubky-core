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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let client = Client::new()?;

    match cli.url.scheme() {
        "https" => {
            unimplemented!();
        }
        "pubky" => {
            let response = client.get(cli.url).send().await?.bytes().await?;

            println!("Got a response: \n {:?}", response);
        }
        _ => {
            panic!("Only https:// and pubky:// URL schemes are supported")
        }
    }

    Ok(())
}
