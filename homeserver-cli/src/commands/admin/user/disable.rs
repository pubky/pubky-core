use crate::commands::admin::context::AdminContext;
use anyhow::Result;
use clap::Args;
use pubky::PublicKey;

#[derive(Args, Debug)]
pub struct DisableArgs {
    pub pubky: PublicKey,
}

pub fn run(context: AdminContext, args: &DisableArgs) -> Result<()> {
    let pk = args.pubky.z32();
    context.client.get(&format!("users/{}", pk))?;
    println!("Disabled user: {}", pk);
    Ok(())
}
