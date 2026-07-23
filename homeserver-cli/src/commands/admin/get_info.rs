use super::error::map_http;
use crate::commands::admin::context::AdminContext;
use anyhow::{Context, Result};
use clap::Args;
use reqwest::blocking::Response;
use serde::Deserialize;

#[derive(Args, Debug)]
pub struct GetInfoArgs {}

#[derive(Debug, Deserialize)]
struct AdminInfoResponse {
    num_users: u64,
    num_disabled_users: u64,
    total_disk_used_mb: f64,
    num_signup_codes: u64,
    num_unused_signup_codes: u64,
}

pub fn run(context: AdminContext, _args: &GetInfoArgs) -> Result<()> {
    let response = context.client.get("info").map_err(map_http)?;
    let info = parse_info(response)?;
    println!("{}", format_info(&info));
    Ok(())
}

fn parse_info(response: Response) -> Result<AdminInfoResponse> {
    response
        .json()
        .context("failed to parse admin info response")
}

fn format_info(info: &AdminInfoResponse) -> String {
    format!(
        "Homeserver Admin Info\n\
         ---------------------\n\
         Users:            {} ({} disabled)\n\
         Disk used:        {:.1} MB\n\
         Signup codes:     {} ({} unused)",
        info.num_users,
        info.num_disabled_users,
        info.total_disk_used_mb,
        info.num_signup_codes,
        info.num_unused_signup_codes,
    )
}
