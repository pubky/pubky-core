use pubky_testnet::{pubky::prelude::*, EphemeralTestnet};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Spin up ephemeral DHT + homeserver
    let testnet = EphemeralTestnet::start().await?;
    let homeserver = testnet.homeserver();

    // Create a random signer and sign up
    let signer = PubkySigner::random()?;
    let session = signer.signup(&homeserver.public_key(), None).await?;

    // Write a file
    session.storage().put("/pub/my.app/hello.txt", "hi").await?;

    // Read it back
    let txt = session
        .storage()
        .get("/pub/my.app/hello.txt")
        .await?
        .text()
        .await?;
    assert_eq!(txt, "hi");

    println!("Roundtrip succeeded: {txt}");
    Ok(())
}
