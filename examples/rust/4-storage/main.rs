mod read;
mod write;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about = "Pubky homeserver storage examples")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Public read: GET data from any user's homeserver (no authentication needed)
    Read(read::Args),
    /// Authenticated write: PUT, GET, and DELETE a file on your own homeserver
    Write(write::Args),
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Read(args) => read::run(args).await,
        Command::Write(args) => write::run(args).await,
    }
}
