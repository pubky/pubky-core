use anyhow::Result;
use clap::Parser;
use futures_util::StreamExt;
use pubky::{EventCursor, EventType, Pubky, PublicKey};
use std::env;

#[derive(Parser, Debug)]
#[command(version, about = "Subscribe to multiple Pubky users' event streams")]
struct Cli {
    /// User public keys (z32 format, 1-50 users)
    #[arg(required = true)]
    users: Vec<String>,
    /// Cursors for each user (comma-separated, optional)
    /// Format: cursor1,cursor2,... (must match number of users if provided)
    #[arg(long)]
    cursors: Option<String>,
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

    // Parse cursors if provided
    let cursors: Vec<Option<EventCursor>> = if let Some(cursor_str) = &args.cursors {
        let cursor_parts: Vec<&str> = cursor_str.split(',').collect();
        if cursor_parts.len() != args.users.len() {
            anyhow::bail!(
                "Number of cursors ({}) must match number of users ({})",
                cursor_parts.len(),
                args.users.len()
            );
        }
        cursor_parts
            .iter()
            .map(|s| {
                if s.trim().is_empty() {
                    Ok(None)
                } else {
                    s.trim()
                        .parse::<u64>()
                        .map(|id| Some(EventCursor::new(id)))
                        .map_err(|e| anyhow::anyhow!("Invalid cursor '{}': {}", s, e))
                }
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![None; args.users.len()]
    };

    // Build event stream subscription
    let mut builder = pubky.event_stream();

    // Add users with their cursors
    for (user_str, cursor) in args.users.iter().zip(cursors.iter()) {
        let user_pubkey = PublicKey::try_from(user_str.as_str())?;
        builder = builder.add_user(&user_pubkey, *cursor)?;
    }

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

    println!("Subscribing to events for {} user(s)", args.users.len());
    for user in &args.users {
        println!("  - {}", user);
    }
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
