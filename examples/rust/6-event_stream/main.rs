use anyhow::Result;
use clap::Parser;
use futures_util::StreamExt;
use pubky::{EventType, Pubky, PublicKey};
use std::env;

#[derive(Parser, Debug)]
#[command(version, about = "Subscribe to a Pubky user's event stream")]
struct Cli {
    /// User public key (z32 format)
    user: String,
    /// Maximum number of events to fetch (omit for unlimited)
    #[arg(short, long)]
    limit: Option<u16>,
    /// Enable live streaming mode
    #[arg(short = 'L', long)]
    live: bool,
    /// Reverse chronological order (newest first)
    #[arg(short, long)]
    reverse: bool,
    /// Filter events by path prefix (e.g., "/pub/posts/")
    #[arg(short, long)]
    path: Option<String>,
    /// Start from this cursor
    #[arg(short, long)]
    cursor: Option<String>,
    /// Use testnet endpoints
    #[arg(long)]
    testnet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(env::var("TRACING").unwrap_or_else(|_| "info".to_string()))
        .init();

    let pubky = if args.testnet {
        Pubky::testnet()?
    } else {
        Pubky::new()?
    };

    let user_pubkey = PublicKey::try_from(args.user.as_str())?;

    // Build event stream subscription
    let mut builder = pubky.event_stream_for(&user_pubkey);

    if let Some(limit) = args.limit {
        builder = builder.limit(limit);
    }

    if args.live {
        builder = builder.live();
    }

    if args.reverse {
        builder = builder.reverse();
    }

    if let Some(path) = args.path {
        builder = builder.path(path);
    }

    if let Some(cursor) = args.cursor {
        builder = builder.cursor(cursor);
    }

    println!("Subscribing to events for user: {}", args.user);
    let mut stream = builder.subscribe().await?;

    // Process events
    while let Some(result) = stream.next().await {
        let event = result?;
        println!(
            "[{}] {} (cursor: {}, hash: {})",
            match event.event_type {
                EventType::Put => "PUT",
                EventType::Delete => "DEL",
            },
            event.path,
            event.cursor,
            event.content_hash.unwrap_or_else(|| "-".to_string())
        );
    }

    Ok(())
}
