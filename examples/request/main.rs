use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use url::Url;

use pubky::PubkyClient;

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

    let client = PubkyClient::builder().build();

    match cli.url.scheme() {
        "https" => {
            unimplemented!();
        }
        "pubky" => {
            let response = client.get(cli.url).await.unwrap();

            println!("Got a response: \n {:?}", response);
        }
        _ => {
            panic!("Only https:// and pubky:// URL schemes are supported")
        }
    }

    Ok(())
}
