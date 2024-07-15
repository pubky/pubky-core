use anyhow::Result;
use pubky_homeserver::Homeserver;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("pubky_homeserver=debug,tower_http=debug")
        .init();

    let server = Homeserver::start().await?;

    server.run_until_done().await?;

    Ok(())
}
