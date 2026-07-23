use super::context::AdminContext;
use super::error::map_http;
use anyhow::{Context, Result};
use clap::Args;

#[derive(Args, Debug)]
pub struct GenerateSignupTokenArgs {}

pub fn run(context: AdminContext, _args: &GenerateSignupTokenArgs) -> Result<()> {
    let token = context
        .client
        .post_json("generate_signup_token", &serde_json::json!({}))
        .map_err(map_http)?
        .text()
        .context("failed to read signup token response")?;

    println!("invite code: {token}");
    Ok(())
}
