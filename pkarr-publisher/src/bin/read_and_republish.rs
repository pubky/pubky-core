//! 
//! Reads `published_secrets.txt` and tries to republish the packets.
//! This is done in a multi-threaded way to improve speed.
//!
//! Run with `cargo run --bin read_and_republish -- --num-records 100 --threads 10`.
//! 

use clap::Parser;
use pkarr::{ClientBuilder, Keypair, PublicKey};
use pkarr_publisher::{
    MultiRepublisher,
    RepublishError, RepublishInfo, RepublisherSettings,
};
use rand::rng;
use rand::seq::SliceRandom;
use std::{
    collections::HashMap,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(author, about = "Reads `published_secrets.txt` and tries to republish the packets.")]
struct Cli {
    /// How many keys should be republished?
    #[arg(long, default_value_t = 100)]
    num_records: usize,

    /// Number of parallel threads
    #[arg(long, default_value_t = 10)]
    threads: u8,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("read_and_republish started.");
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



    println!("Read published_secrets.txt");
    let mut published_keys = read_keys();
    println!("Read {} keys", published_keys.len());

    println!(
        "Take a random sample of {} keys to republish.",
        cli.num_records
    );
    let mut rng = rng();
    published_keys.shuffle(&mut rng);
    let keys: Vec<Keypair> = published_keys
        .into_iter()
        .take(cli.num_records)
        .collect();

    run_churn_loop(keys, cli.threads).await;

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

async fn run_churn_loop(keys: Vec<Keypair>, thread_count: u8) {
    let public_keys = keys.into_iter().map(|key| key.public_key()).collect();

    let mut builder = ClientBuilder::default();
    builder.no_relays();
    let republisher = MultiRepublisher::new_with_settings( RepublisherSettings::new(), Some(builder));

    println!("Republish keys. Hold on...");
    let start = Instant::now();
    let results: HashMap<PublicKey, Result<RepublishInfo, RepublishError>> = republisher
        .run(public_keys, thread_count)
        .await
        .unwrap();

    let elapsed_seconds = start.elapsed().as_secs_f32();
    let keys_per_s = results.len() as f32 / elapsed_seconds;
    tracing::info!(
        "Processed {} keys within {elapsed_seconds:.2}s. {keys_per_s:.2} keys/s.",
        results.len()
    );

    let success: HashMap<&PublicKey, &Result<RepublishInfo, RepublishError>> =
        results.iter().filter(|(_, val)| val.is_ok()).collect();
    let missing: HashMap<&PublicKey, &Result<RepublishInfo, RepublishError>> = results
        .iter()
        .filter(|(_, val)| {
            if let Err(e) = val {
                return e.is_missing();
            }
            return false;
        })
        .collect();
    let failed: HashMap<&PublicKey, &Result<RepublishInfo, RepublishError>> = results
        .iter()
        .filter(|(_, val)| {
            if let Err(e) = val {
                return e.is_publish_failed();
            }
            return false;
        })
        .collect();

    tracing::info!(
        "{} success, {} missing, {} failed.",
        success.len(),
        missing.len(),
        failed.len()
    );

    tracing::info!("Republishing finished.");
}
