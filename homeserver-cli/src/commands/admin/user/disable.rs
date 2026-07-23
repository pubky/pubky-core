use super::error::map_http;
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
    context
        .client
        .post(&format!("users/{}", pk))
        .map_err(map_http)?;
    println!("disabled user: {}", pk);
    Ok(())
}
