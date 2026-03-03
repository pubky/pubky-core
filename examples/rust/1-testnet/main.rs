use clap::Parser;
use pubky_testnet::{pubky::Keypair, EphemeralTestnet};

#[derive(Parser)]
struct Args {
    /// Use an external PostgreSQL instance instead of embedded postgres.
    /// Connects to TEST_PUBKY_CONNECTION_STRING env var if set,
    /// otherwise defaults to postgres://postgres:postgres@localhost:5432/postgres
    #[arg(long)]
    external_postgres: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    #[allow(unused_variables)]
    let args = Args::parse();

    // Spin up ephemeral DHT + homeserver with minimal config
    #[allow(unused_mut)]
    let mut builder = EphemeralTestnet::builder();

    #[cfg(feature = "embedded-postgres")]
    let builder = if !args.external_postgres {
        builder.with_embedded_postgres()
    } else {
        builder
    };

    let testnet = builder.build().await?;
    let homeserver = testnet.homeserver_app();

    // Intantiate a Pubky SDK wrapper that uses this testnet's preconfigured client for transport
    let pubky = testnet.sdk()?;

    // Create a random signer and sign up
    let session = pubky
        .signer(Keypair::random())
        .signup(&homeserver.public_key(), None)
        .await?;

    // Write a file
    session
        .storage()
        .put("/pub/my-cool-app/hello.txt", "hi")
        .await?;

    // Read it back
    let txt = session
        .storage()
        .get("/pub/my-cool-app/hello.txt")
        .await?
        .text()
        .await?;
    assert_eq!(txt, "hi");

    println!("Roundtrip succeeded: {txt}");
    Ok(())
}
