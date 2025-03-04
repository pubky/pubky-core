//! Publish and save the published public keys in a file
//! so they can be reused in other experiments.
//!
//! Run with `cargo run --bin main_publish_and_save`.

use clap::Parser;
use pkarr::{dns::Name, mainline::Dht, Client, Keypair, PublicKey};
use rand::seq::SliceRandom;
use rand::rng;
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
#[command(author, version, about)]
struct Cli {
    /// Number of parallel threads
    #[arg(long, default_value_t = 12)]
    threads: usize,
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
        std::process::exit(0);
    })
    .expect("Error setting Ctrl+C handler");

    println!("Press Ctrl+C to stop...");

    let cli = Cli::parse();
    
    let num_verify_keys = cli.num_verify_keys;
    info!("Publish {} records. Verify: {num_verify_keys}", cli.num_records);
    let published_keys = publish_parallel(cli.num_records, cli.threads, &ctrlc_pressed).await;

    // Turn into a hex list and write to file
    let pubkeys = published_keys
        .clone().into_iter()
        .map(|key| {
            let secret = key.secret_key();
            let h = hex::encode(secret);
            h
        })
        .collect::<Vec<_>>();
    let pubkeys_str = pubkeys.join("\n");
    std::fs::write("published_secrets.txt", pubkeys_str).unwrap();
    info!("Successfully wrote secrets keys to published_secrets.txt");

    if num_verify_keys > 0 {
        info!("Verify {num_verify_keys} published keys randomly.");
        verify_published(&published_keys, num_verify_keys).await;
    };
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


// Publishes x number of packets. Checks if they are actually available
async fn publish_records(num_records: usize, thread_id: usize) -> Vec<Keypair> {
    let client = Client::builder().no_relays().build().unwrap();
    let dht = client.dht().unwrap();
    dht.clone().as_async().bootstrapped().await;
    let mut records = vec![];

    for i in 0..num_records {
        let instant = Instant::now();
        let key = Keypair::random();
        let packet = pkarr::SignedPacketBuilder::default().cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600).build(&key).unwrap();
        if let Err(e) = client.publish(&packet, None).await {
            tracing::error!("Failed to publish {} record: {e:?}", key.public_key());
            continue;
        }
        let publish_time = instant.elapsed().as_millis();
        tracing::info!("- t{thread_id:<2} {i:>3}/{num_records} Published {} within {publish_time}ms", key.public_key());
        records.push(key);
    }
    records
}

async fn verify_published(keys: &Vec<Keypair>, count: usize) {
    // Shuffle and take {count} elements to verify.
    let mut keys = keys.clone();
    let mut rng = rng();
    keys.shuffle(&mut rng);
    let keys: Vec<Keypair> = keys.into_iter().take(count).collect();

    let client = Client::builder().no_relays().build().unwrap();
    let dht = client.dht().unwrap();
    dht.clone().as_async().bootstrapped().await;
    for (i, key) in keys.into_iter().enumerate() {
        let nodes_count = count_dht_nodes_storing_packet(&key.public_key(), &dht).await;
        tracing::info!("- {i}/{count} Verify {} found on {nodes_count} nodes.", key.public_key());
    }
}


/// Queries the public key and returns how many nodes responded with the packet.
pub async fn count_dht_nodes_storing_packet(pubkey: &PublicKey, client: &Dht) -> u8 {
    let c = client.clone();
    let p = pubkey.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let stream = c.get_mutable(p.as_bytes(), None, None);
        let mut response_count: u8 = 0;
    
        for _ in stream {
            response_count += 1;
        }
    
        response_count
    });

    handle.await.unwrap()
}