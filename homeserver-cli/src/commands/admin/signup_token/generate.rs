use crate::commands::admin::context::AdminContext;
use crate::commands::admin::signup_token::error::map_http;
use crate::helpers::quota::{Quota, QuotaUpdate, RateLimit};
use anyhow::{Context, Result};
use clap::Args;

#[derive(Args, Debug)]
pub struct GenerateArgs {
    #[arg(long)]
    pub storage_quota_mb: Option<Quota>,

    #[arg(long)]
    pub rate_read: Option<RateLimit>,

    #[arg(long)]
    pub rate_write: Option<RateLimit>,
}

pub fn run(context: AdminContext, args: &GenerateArgs) -> Result<()> {
    let body = QuotaUpdate {
        storage_quota_mb: args.storage_quota_mb,
        rate_read: args.rate_read.clone(),
        rate_write: args.rate_write.clone(),
    };

    let token = context
        .client
        .post_json("generate_signup_token", &body)
        .map_err(map_http)?
        .text()
        .context("failed to read signup token response")?;

    println!("invite code: {token}");
    Ok(())
}
