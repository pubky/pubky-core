use crate::commands::admin::context::AdminContext;
use crate::helpers::quota::{Quota, QuotaUpdate, RateLimit};
use anyhow::Result;
use clap::{ArgGroup, Args};
use pubky::PublicKey;

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("quota_fields")
        .required(true)
        .multiple(true)
        .args(["storage_quota_mb", "rate_read", "rate_write"]),
))]
pub struct SetArgs {
    pub pubky: PublicKey,

    #[arg(long)]
    pub storage_quota_mb: Option<Quota>,

    #[arg(long)]
    pub rate_read: Option<RateLimit>,

    #[arg(long)]
    pub rate_write: Option<RateLimit>,
}

pub fn run(context: AdminContext, args: &SetArgs) -> Result<()> {
    let pk = args.pubky.z32();

    let body = QuotaUpdate {
        storage_quota_mb: args.storage_quota_mb,
        rate_read: args.rate_read.clone(),
        rate_write: args.rate_write.clone(),
    };

    context
        .client
        .patch_json(&format!("users/{}/quota", pk), &body)?;

    println!("Updated quota for user: {}", pk);

    Ok(())
}
