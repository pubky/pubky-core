use clap::Args;
use anyhow::Result;
use url::Url;

#[derive(Args, Debug)]
pub struct GetInfoArgs {
}

pub fn run(admin_endpoint: Url, admin_password: String, _args: &GetInfoArgs) -> Result<()> {
    println!("{} {}",admin_endpoint , admin_password);
    println!("get_info_mock");
    Ok(())
}
