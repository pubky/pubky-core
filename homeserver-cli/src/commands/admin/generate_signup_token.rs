use super::context::AdminContext;
use anyhow::{Context, Result};
use clap::Args;

#[derive(Args, Debug)]
pub struct GenerateSignupTokenArgs {}

pub fn run(context: AdminContext, _args: &GenerateSignupTokenArgs) -> Result<()> {
    let token = context
        .client
        .post_json("generate_signup_token", &serde_json::json!({}))?
        .text()
        .context("failed to read signup token response")?;

    println!("Invite code: {token}");
    Ok(())
}
