use anyhow::Result;
use pubky_homeserver::Homeserver;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let server = Homeserver::start().await?;

    server.run_until_done().await?;

    Ok(())
}
