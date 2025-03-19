//!
//! Publishs packets with random keys and saves the published public keys in a file
//! so they can be reused in other experiments.
//!
//! Run with `cargo run --example publish_and_save -- --num-records 100 --threads 6`.
//!

use clap::Parser;

use pkarr::{dns::Name, Client, Keypair, SignedPacket};
use pkarr_republisher::{ResilientClient, RetrySettings};
use std::{
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    author,
    about = "Publish random packets and save them in `published_secrets.txt`."
)]
struct Cli {
    /// Number of records to publish
    #[arg(long, default_value_t = 100)]
    num_records: usize,

    /// Number of parallel threads
    #[arg(long, default_value_t = 6)]
    threads: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("publish_and_save started.");

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

    info!("Publish {} records. Verify", cli.num_records);
    let published_keys = publish_parallel(cli.num_records, cli.threads, &ctrlc_pressed).await;

    // Turn into a hex list and write to file
    let pubkeys = published_keys
        .clone()
        .into_iter()
        .map(|key| {
            let secret = key.secret_key();
            hex::encode(secret)
        })
        .collect::<Vec<_>>();
    let pubkeys_str = pubkeys.join("\n");
    std::fs::write("published_secrets.txt", pubkeys_str).unwrap();
    info!("Successfully wrote secrets keys to published_secrets.txt");

    Ok(())
}

// Publish records in multiple threads.
async fn publish_parallel(
    num_records: usize,
    threads: usize,
    ctrlc_pressed: &Arc<AtomicBool>,
) -> Vec<Keypair> {
    let start = Instant::now();
    let mut handles = vec![];
    for thread_id in 0..threads {
        let handle = tokio::spawn(async move {
            tracing::info!("Started thread t{thread_id}");
            publish_records(num_records / threads, thread_id).await
        });
        handles.push(handle);
    }

    loop {
        let all_finished = handles
            .iter()
            .map(|handle| handle.is_finished())
            .reduce(|a, b| a && b)
            .unwrap();
        if all_finished {
            break;
        }
        if ctrlc_pressed.load(Ordering::Relaxed) {
            break;
        }
        sleep(Duration::from_millis(250)).await;
    }

    if ctrlc_pressed.load(Ordering::Relaxed) {
        process::exit(0);
    }

    let mut all_result = vec![];
    for handle in handles {
        let keys = handle.await.unwrap();
        all_result.extend(keys);
    }

    let rate = all_result.len() as f64 / start.elapsed().as_secs() as f64;
    tracing::info!(
        "Published {} keys in {} seconds at {rate:.2} keys/s",
        all_result.len(),
        start.elapsed().as_secs()
    );

    all_result
}

// Publishes x number of packets. Checks if they are actually available.
async fn publish_records(num_records: usize, thread_id: usize) -> Vec<Keypair> {
    let client = Client::builder().no_relays().build().unwrap();
    let rclient = ResilientClient::new_with_client(client, RetrySettings::default()).unwrap();
    let mut records = vec![];

    for i in 0..num_records {
        let instant = Instant::now();
        let key = Keypair::random();
        let packet: SignedPacket = pkarr::SignedPacketBuilder::default()
            .cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600)
            .build(&key)
            .unwrap();
        let result = rclient.publish(packet, None).await;
        let elapsed_time = instant.elapsed().as_millis();
        if result.is_ok() {
            let info = result.unwrap();
            tracing::info!("- t{thread_id:<2} {i:>3}/{num_records} Published {} within {elapsed_time}ms to {} nodes {} attempts", key.public_key(), info.published_nodes_count, info.attempts_needed);
            records.push(key);
        } else {
            let e = result.unwrap_err();
            tracing::error!("Failed to publish {} record: {e:?}", key.public_key());
            continue;
        }
    }
    records
}
