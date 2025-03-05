//! Publish and save the published public keys in a file
//! so they can be reused in other experiments.
//!
//! Run with `cargo run --bin main_publish_and_save`.

use clap::Parser;
use pkarr::Keypair;
use pkarr_publisher::pkarr_publisher::PkarrRepublisher;
use rand::seq::SliceRandom;
use rand::rng;
use std::{
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    }
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Number of parallel threads
    #[arg(long, default_value_t = 10)]
    republish_count: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("read_and_republish started.");
    // Initialize tracing

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(LevelFilter::INFO.into()))
        .init();

    // Set up the Ctrl+C handler
    let ctrlc_pressed: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let r = ctrlc_pressed.clone();
    ctrlc::set_handler(move || {
        r.store(true, Ordering::SeqCst);
        println!("Ctrl+C detected, shutting down...");
        process::exit(0);
    })
    .expect("Error setting Ctrl+C handler");

    println!("Press Ctrl+C to stop...");

    let cli = Cli::parse();

    println!("Read published_secrets.txt");
    let mut published_keys = read_keys();
    println!("Read {} keys", published_keys.len());

    println!("Take a random sample of {} keys to republish.", cli.republish_count);
    let mut rng = rng();
    published_keys.shuffle(&mut rng);
    let keys: Vec<Keypair> = published_keys.into_iter().take(cli.republish_count).collect();

    run_churn_loop(keys).await;

    Ok(())
}

fn read_keys() -> Vec<Keypair> {
    let secret_srs = std::fs::read_to_string("published_secrets.txt").expect("File not found");
    let keys = secret_srs.lines().map(|line| line.to_string()).collect::<Vec<_>>();
    keys.into_iter().map(|key| {
        let secret = hex::decode(key).expect("invalid hex");
        let secret: [u8; 32] = secret.try_into().unwrap();
        Keypair::from_secret_key(&secret)
    }).collect::<Vec<_>>()
}


async fn run_churn_loop(keys: Vec<Keypair>) {
    let public_keys = keys.into_iter().map(|key| key.public_key()).collect();

    let republisher = PkarrRepublisher::new().unwrap();
    republisher.wait_until_dht_is_bootstrap().await;

    println!("Republish keys. Hold on...");
    let _ = republisher.run_parallel(public_keys, 8).await;

    println!("Republishing finished.");
}

