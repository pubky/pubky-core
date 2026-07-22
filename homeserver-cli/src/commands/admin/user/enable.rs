use crate::commands::admin::context::AdminContext;
use anyhow::Result;
use clap::Args;
use pubky::PublicKey;

#[derive(Args, Debug)]
pub struct EnableArgs {
    pub pubky: PublicKey,
}

pub fn run(context: AdminContext, args: &EnableArgs) -> Result<()> {
    let pk = args.pubky.z32();
    context.client.post(&format!("users/{}/enable", pk))?;
    println!("Enabled user: {}", pk);
    Ok(())
}
