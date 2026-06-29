use anyhow::Result;
use clap::Parser;
use pubky::Pubky;

#[derive(Parser, Debug)]
pub struct Args {
    /// Pubky resource (e.g. `pubky<user>/pub/...` or `pubky://<user>/pub/...`)
    pub resource: String,
    /// Use testnet mode
    #[clap(long)]
    pub testnet: bool,
}

pub async fn run(args: Args) -> Result<()> {
    let storage = if args.testnet {
        Pubky::testnet()?.public_storage()
    } else {
        Pubky::new()?.public_storage()
    };

    let response = storage.get(args.resource).await?;

    println!("< Response:");
    println!("< {:?} {}", response.version(), response.status());
    for (name, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            println!("< {name}: {v}");
        }
    }

    let bytes = response.bytes().await?;

    match String::from_utf8(bytes.to_vec()) {
        Ok(string) => println!("<\n{}", string),
        Err(_) => println!("<\n{:?}", bytes),
    }

    Ok(())
}
