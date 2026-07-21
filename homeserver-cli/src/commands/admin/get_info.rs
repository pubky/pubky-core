use super::settings::AdminSettings;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug)]
pub struct GetInfoArgs {}

pub fn run(settings: AdminSettings, _args: &GetInfoArgs) -> Result<()> {
    println!("{} {}", settings.endpoint, settings.password);
    println!("get_info_mock");
    Ok(())
}
