//!
//! Reads `published_secrets.txt` and outputs how many nodes store this public key.
//! Run `publish_and_save` first to publish some packets to verify
//! Freshly stored once should have 15+.
//! <10 is ready for a republish.
//! 0 = Packet unavailable.
//!
//! Run with `cargo run --example read_and_verify -- --num_keys 20`
//!

use clap::Parser;
use pkarr::{Client, Keypair};
use pkarr_republisher::{ResilientClient, RetrySettings};
use rand::rng;
use rand::seq::SliceRandom;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, about = "Verify pkarr packets on the DHT.")]
struct Cli {
    /// Verify x keys by checking how many nodes it was stored on.
    #[arg(long, default_value_t = 20)]
    num_keys: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("read_and_verify started.");

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(LevelFilter::INFO.into()))
        .init();

    // Set up the Ctrl+C handler
    let ctrlc_pressed: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let r = ctrlc_pressed.clone();
    ctrlc::set_handler(move || {
        r.store(true, Ordering::SeqCst);
        println!("Ctrl+C detected, shutting down...");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl+C handler");

    println!("Press Ctrl+C to stop...");

    println!("Read published_secrets.txt");
    let published_keys = read_keys();
    println!("Read {} keys", published_keys.len());

    let num_verify_keys = cli.num_keys;
    info!("Randomly verify: {num_verify_keys} keys");
    verify_published(&published_keys, num_verify_keys).await;
    Ok(())
}

fn read_keys() -> Vec<Keypair> {
    let secret_srs = std::fs::read_to_string("published_secrets.txt").expect("File not found");
    let keys = secret_srs
        .lines()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    keys.into_iter()
        .map(|key| {
            let secret = hex::decode(key).expect("invalid hex");
            let secret: [u8; 32] = secret.try_into().unwrap();
            Keypair::from_secret_key(&secret)
        })
        .collect::<Vec<_>>()
}

async fn verify_published(keys: &[Keypair], count: usize) {
    // Shuffle and take {count} elements to verify.
    let mut keys: Vec<Keypair> = keys.to_owned();
    let mut rng = rng();
    keys.shuffle(&mut rng);
    let keys: Vec<Keypair> = keys.into_iter().take(count).collect();

    let client = Client::builder().no_relays().build().unwrap();
    let rclient = ResilientClient::new_with_client(client, RetrySettings::new());
    let mut success = 0;
    let mut warn = 0;
    let mut error = 0;
    for (i, key) in keys.into_iter().enumerate() {
        let nodes_count = rclient.verify_node_count(&key.public_key()).await;
        if nodes_count == 0 {
            tracing::error!(
                "- {i}/{count} Verify {} found on {nodes_count} nodes.",
                key.public_key()
            );
            error += 1;
        } else if nodes_count < 5 {
            tracing::warn!(
                "- {i}/{count} Verify {} found on {nodes_count} nodes.",
                key.public_key()
            );
            warn += 1;
        } else {
            tracing::info!(
                "- {i}/{count} Verify {} found on {nodes_count} nodes.",
                key.public_key()
            );
            success += 1;
        }
    }
    println!("Success: {success}, Warn: {warn}, Error: {error}");
}
