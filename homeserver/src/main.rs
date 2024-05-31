use anyhow::Result;
use pk_homeserver::Homeserver;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("pk_homeserver=debug,tower_http=debug".to_string())
        .init();

    let server = Homeserver::start(Default::default()).await?;

    server.run_until_done().await?;

    Ok(())
}
