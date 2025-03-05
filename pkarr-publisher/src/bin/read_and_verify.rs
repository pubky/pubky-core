//! Reads `published_secrets.txt` and verifies if the keys have been published correctly.

use clap::Parser;
use pkarr::{dns::Name, mainline::{Dht}, Client, Keypair, PublicKey, SignedPacket};
use rand::seq::SliceRandom;
use rand::rng;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    }
};

use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;
use futures_lite::StreamExt;



#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Verify x keys by checking how many nodes it was stored on.
    #[arg(long, default_value_t = 20)]
    num_keys: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("read_and_verify started.");
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
    let keys = secret_srs.lines().map(|line| line.to_string()).collect::<Vec<_>>();
    keys.into_iter().map(|key| {
        let secret = hex::decode(key).expect("invalid hex");
        let secret: [u8; 32] = secret.try_into().unwrap();
        Keypair::from_secret_key(&secret)
    }).collect::<Vec<_>>()
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
    let c = client.clone().as_async();
    let mut response_count: u8 = 0;
    let mut stream = c.get_mutable(pubkey.as_bytes(), None, None);
    while let Some(_) = stream.next().await {
        response_count += 1;
    }

    response_count
}