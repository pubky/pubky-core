use super::error::map_http;
use crate::commands::admin::context::AdminContext;
use crate::helpers::quota::UserQuota;
use anyhow::{Context, Result};
use clap::Args;
use pubky::PublicKey;

#[derive(Args, Debug)]
pub struct GetArgs {
    pub pubky: PublicKey,
}

pub fn run(context: AdminContext, args: &GetArgs) -> Result<()> {
    let pk = args.pubky.z32();

    let response = context
        .client
        .get(&format!("users/{}/quota", pk))
        .map_err(map_http)?;

    let quota: UserQuota = response.json().context("failed to parse quota response")?;

    println!("Quota for user {}:", pk);
    println!("  effective:");
    println!("    storage_quota_mb: {}", quota.effective.storage_quota_mb);
    println!("    rate_read:        {}", quota.effective.rate_read);
    println!("    rate_write:       {}", quota.effective.rate_write);

    Ok(())
}
