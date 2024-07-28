use anyhow::Result;
use pkarr::mainline::Testnet;
use pubky_homeserver::Homeserver;

use clap::Parser;

#[derive(Parser, Debug)]
struct Cli {
    /// [tracing_subscriber::EnvFilter]
    #[clap(short, long)]
    tracing_env_filter: Option<String>,
    #[clap(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            args.tracing_env_filter
                .unwrap_or("pubky_homeserver=debug,tower_http=debug".to_string()),
        )
        .init();

    let server = if args.testnet {
        let testnet = Testnet::new(3);

        Homeserver::start_test(&testnet).await?
    } else {
        Homeserver::start(Default::default()).await?
    };

    server.run_until_done().await?;

    Ok(())
}
